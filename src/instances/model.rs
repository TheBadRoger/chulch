use crate::{LaunchArguments, Loader};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum Version {
    V1,
}

impl TryFrom<u32> for Version {
    type Error = String;
    fn try_from(n: u32) -> std::result::Result<Self, String> {
        match n {
            1 => Ok(Self::V1),
            _ => Err(format!("unsupported schemaVersion {n}")),
        }
    }
}
impl From<Version> for u32 {
    fn from(_: Version) -> Self {
        1
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Generation {
    pub(crate) schema_version: Version,
    pub(crate) minecraft: String,
    #[serde(default)]
    pub(crate) loader: Loader,
    #[serde(alias = "fabric")]
    pub(crate) loader_version: String,
    pub(crate) platform: String,
    pub(crate) java_major: u32,
    pub(crate) main_class: String,
    pub(crate) argument_profile: String,
    #[serde(default)]
    pub(crate) arguments: LaunchArguments,
    #[serde(default)]
    pub(crate) quick_play: Option<bool>,
    pub(crate) asset_index: String,
    pub(crate) layers: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Profile {
    pub(super) schema_version: Version,
    pub(super) generation: String,
}
