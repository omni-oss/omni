use clap::ArgAction;
use omni_remote_cache_storage::impls::BasicS3Config;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, clap::Args, Clone, PartialEq, Eq)]
pub struct S3BackendConfig {
    #[arg(
        long = "s3.endpoint",
        default_value = "",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_ENDPOINT",
        help = "The endpoint to use for S3"
    )]
    pub endpoint: String,

    #[arg(
        long = "s3.access-key-id",
        default_value = "",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_ACCESS_KEY_ID",
        help = "The access key ID to use for S3"
    )]
    pub access_key_id: String,

    #[arg(
        long = "s3.secret-access-key",
        default_value = "",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_SECRET_ACCESS_KEY",
        help = "The secret access key to use for S3"
    )]
    pub secret_access_key: String,

    #[arg(
        long = "s3.bucket",
        default_value = "omni-remote-cache",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_BUCKET",
        help = "The default s3 bucket to use"
    )]
    pub default_bucket: String,

    #[arg(
        long = "s3.region",
        default_value = "auto",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_REGION",
        help = "The region to use for S3"
    )]
    pub region: String,

    #[arg(
        long = "s3.force-path-style",
        action = ArgAction::SetTrue,
        default_value = "false",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_FORCE_PATH_STYLE"
    )]
    pub force_path_style: bool,

    #[arg(
        long = "s3.multi-bucket",
        action = ArgAction::SetTrue,
        default_value = "false",
        env = "OMNI_REMOTE_CACHE_SERVICE_S3_MULTI_BUCKET",
        help = "If set, the bucket name will be the container name"
    )]
    pub multi_bucket: bool,
}

impl S3BackendConfig {
    #[allow(unused)]
    pub fn into_basig_config(self) -> BasicS3Config {
        BasicS3Config {
            endpoint: self.endpoint,
            access_key_id: self.access_key_id,
            secret_access_key: self.secret_access_key,
            default_bucket: self.default_bucket,
            default_container: "default".to_string(),
            region: self.region,
            force_path_style: self.force_path_style,
            multi_bucket: self.multi_bucket,
        }
    }

    #[allow(unused)]
    pub fn to_basig_config(&self) -> BasicS3Config {
        let config = (*self).clone();

        config.into_basig_config()
    }
}
