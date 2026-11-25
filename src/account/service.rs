use super::Session;
use crate::Player;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const DEVICE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const USER: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const LICENSE: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

pub(super) struct Service {
    http: ureq::Agent,
}

#[derive(Deserialize)]
pub(super) struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn read(mut response: ureq::http::Response<ureq::Body>) -> Result<Self> {
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_json();
        Ok(Self {
            status,
            body: if (200..300).contains(&status) {
                body.context("Invalid authentication service response")?
            } else {
                body.unwrap_or(Value::Null)
            },
        })
    }

    fn success(self, step: &str) -> Result<Value> {
        if !(200..300).contains(&self.status) {
            let hint = match self.body.get("error").and_then(Value::as_str) {
                Some("invalid_grant" | "interaction_required") => {
                    "Run mcdocker account login again."
                }
                Some("authorization_declined" | "access_denied") => "Authorization was declined.",
                Some("expired_token") => "The sign-in code expired. Start login again.",
                Some("invalid_client" | "unauthorized_client") => {
                    "Check the client ID and public client configuration."
                }
                _ => match (step, self.status, self.body["XErr"].as_u64()) {
                    ("Xbox authorization", _, Some(2148916233)) => {
                        "Create an Xbox profile for this Microsoft account."
                    }
                    ("Xbox authorization", _, Some(2148916238)) => {
                        "Check Xbox family and age permissions."
                    }
                    ("Minecraft sign-in", 403, _) => {
                        "Check that this client ID has Minecraft Java API access."
                    }
                    ("Minecraft profile", 404, _) => {
                        "Set up a Minecraft Java profile before signing in."
                    }
                    _ => "Check the account permissions and try again.",
                },
            };
            bail!("{step} failed (HTTP {}). {hint}", self.status);
        }
        Ok(self.body)
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("Missing {key} in authentication response"))
}

impl Service {
    pub fn new() -> Self {
        Self {
            http: ureq::Agent::config_builder()
                .https_only(true)
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(40)))
                .build()
                .into(),
        }
    }

    fn form(&self, url: &str, params: &[(&str, &str)]) -> Result<Reply> {
        Reply::read(
            self.http
                .post(url)
                .send_form(params.iter().copied())
                .context("Cannot reach Microsoft sign-in")?,
        )
    }

    pub fn login(&self, client: &str) -> Result<Tokens> {
        let device = self
            .form(DEVICE, &[("client_id", client), ("scope", SCOPE)])?
            .success("Microsoft device authorization")?;
        let lifetime = device["expires_in"]
            .as_u64()
            .context("Missing sign-in code lifetime")?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(lifetime))
            .context("Invalid sign-in code lifetime")?;
        let mut interval = device["interval"].as_u64().unwrap_or(5).max(1);
        let code = field(&device, "device_code")?;
        eprintln!(
            "Open {} and enter code {} to sign in.",
            field(&device, "verification_uri")?,
            field(&device, "user_code")?
        );
        loop {
            let delay = Duration::from_secs(interval);
            ensure!(
                deadline.saturating_duration_since(Instant::now()) > delay,
                "Sign-in code expired. Run login again."
            );
            std::thread::sleep(delay);
            let reply = self.form(
                TOKEN,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", client),
                    ("device_code", code),
                ],
            )?;
            if reply.status == 400 {
                match reply.body["error"].as_str() {
                    Some("authorization_pending") => continue,
                    Some("slow_down") => {
                        interval = interval.saturating_add(5);
                        continue;
                    }
                    _ => {}
                }
            }
            return Ok(serde_json::from_value(reply.success("Microsoft sign-in")?)?);
        }
    }

    pub fn refresh(&self, client: &str, refresh: &str) -> Result<Tokens> {
        let reply = self.form(
            TOKEN,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", client),
                ("scope", SCOPE),
            ],
        )?;
        Ok(serde_json::from_value(
            reply.success("Microsoft token refresh")?,
        )?)
    }

    fn post(&self, url: &str, step: &str, payload: Value) -> Result<Value> {
        Reply::read(
            self.http
                .post(url)
                .header("Accept", "application/json")
                .send_json(&payload)
                .with_context(|| format!("Cannot reach {step} service"))?,
        )?
        .success(step)
    }

    fn get(&self, url: &str, step: &str, token: &str) -> Result<Value> {
        Reply::read(
            self.http
                .get(url)
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .with_context(|| format!("Cannot reach {step} service"))?,
        )?
        .success(step)
    }

    pub fn minecraft(&self, client: &str, access: &str) -> Result<Session> {
        let user = self.post(USER,"Xbox sign-in",json!({"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT",
            "Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={access}")}}))?;
        let xsts = self.post(
            XSTS,
            "Xbox authorization",
            json!({"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT",
            "Properties":{"SandboxId":"RETAIL","UserTokens":[field(&user,"Token")?]}}),
        )?;
        let claims = &xsts["DisplayClaims"]["xui"][0];
        let identity = format!(
            "XBL3.0 x={};{}",
            field(claims, "uhs")?,
            field(&xsts, "Token")?
        );
        let mc = self.post(
            LOGIN,
            "Minecraft sign-in",
            json!({"identityToken":identity}),
        )?;
        let token = field(&mc, "access_token")?;
        let license = self.get(LICENSE, "Minecraft license", token)?;
        ensure!(
            licensed(&license),
            "This account has no active Minecraft Java Edition entitlement."
        );
        let profile = self.get(PROFILE, "Minecraft profile", token)?;
        let id = field(&profile, "id")?;
        ensure!(
            id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid Minecraft profile UUID"
        );
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &id[..8],
            &id[8..12],
            &id[12..16],
            &id[16..20],
            &id[20..]
        );
        let name = field(&profile, "name")?.to_string();
        let xuid = claims["xid"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| token_xuid(token))
            .unwrap_or_default();
        Ok(Session {
            player: Player { name, uuid },
            access: token.to_owned(),
            client: client.to_owned(),
            xuid,
        })
    }
}

fn licensed(reply: &Value) -> bool {
    reply["items"].as_array().is_some_and(|items| {
        items.iter().any(|i| {
            matches!(
                i["name"].as_str(),
                Some("game_minecraft" | "product_minecraft")
            )
        })
    })
}

// The token came directly from Minecraft over TLS. This claim is only a launch argument.
fn token_xuid(token: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let value = &claims["xuid"];
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::{
        Body, SendBody,
        http::{Request, Response},
        middleware::MiddlewareNext,
    };

    // Exercise the real HTTP request construction using synthetic provider responses.
    fn provider(
        request: Request<SendBody>,
        _: MiddlewareNext,
    ) -> Result<Response<Body>, ureq::Error> {
        let (head, body) = request.into_parts();
        let text = std::io::read_to_string(body.into_reader())?;
        let uri = head.uri.to_string();
        let reply = match uri.as_str() {
            DEVICE => {
                assert!(text.contains("client_id=fixture"));
                json!({"device_code":"fixture-device","user_code":"TEST","verification_uri":"https://microsoft.com/link","interval":1,"expires_in":60})
            }
            TOKEN => {
                assert!(
                    text.contains("device_code=fixture-device")
                        || text.contains("refresh_token=fixture-refresh")
                );
                json!({"access_token":"fixture-msa","refresh_token":"fixture-refresh"})
            }
            USER => {
                assert_eq!(
                    serde_json::from_str::<Value>(&text).unwrap()["Properties"]["RpsTicket"],
                    "d=fixture-msa"
                );
                json!({"Token":"fixture-xbl"})
            }
            XSTS => {
                assert_eq!(
                    serde_json::from_str::<Value>(&text).unwrap()["Properties"]["UserTokens"],
                    json!(["fixture-xbl"])
                );
                json!({"Token":"fixture-xsts","DisplayClaims":{"xui":[{"uhs":"123","xid":"456"}]}})
            }
            LOGIN => {
                assert_eq!(
                    serde_json::from_str::<Value>(&text).unwrap()["identityToken"],
                    "XBL3.0 x=123;fixture-xsts"
                );
                json!({"access_token":"fixture-mc"})
            }
            LICENSE | PROFILE => {
                assert_eq!(head.headers["Authorization"], "Bearer fixture-mc");
                if uri == LICENSE {
                    json!({"items":[{"name":"game_minecraft"}]})
                } else {
                    json!({"name":"TestPlayer","id":"0123456789abcdef0123456789abcdef"})
                }
            }
            _ => panic!("Unexpected authentication endpoint"),
        };
        Ok(Response::new(Body::builder().data(reply.to_string())))
    }

    #[test]
    fn device_signin_and_refresh_produce_a_verified_game_identity() -> Result<()> {
        let service = Service {
            http: ureq::Agent::config_builder()
                .middleware(provider)
                .build()
                .into(),
        };
        let first = service.login("fixture")?;
        let refreshed = service.refresh("fixture", first.refresh_token.as_deref().unwrap())?;
        let session = service.minecraft("fixture", &refreshed.access_token)?;
        let mut args = [
            "--username",
            "OfflineFake",
            "--accessToken=0",
            "--userType",
            "legacy",
            "--width",
            "800",
        ]
        .map(String::from)
        .to_vec();
        session.set_game_identity(&mut args);
        assert_eq!(
            args,
            [
                "--username",
                "TestPlayer",
                "--uuid",
                "01234567-89ab-cdef-0123-456789abcdef",
                "--accessToken",
                "fixture-mc",
                "--userType",
                "msa",
                "--clientId",
                "fixture",
                "--xuid",
                "456",
                "--width",
                "800"
            ]
        );
        Ok(())
    }

    #[test]
    fn requires_a_java_entitlement_and_redacts_provider_errors() {
        assert!(!licensed(&json!({"items":[{"name":"other_game"}]})));
        assert!(!licensed(&json!({"items":[]})));
        let error = Reply {
            status: 400,
            body: json!({"error":"invalid_grant","error_description":"sensitive details"}),
        };
        let message = error
            .success("Microsoft token refresh")
            .unwrap_err()
            .to_string();
        assert!(message.contains("login again") && !message.contains("sensitive details"));
    }
}
