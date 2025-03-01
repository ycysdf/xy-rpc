use xy_rpc::{formats::JsonFormat, rpc_service};

pub const PORT: u16 = 30002;
pub type FormatType = JsonFormat;

#[rpc_service]
pub trait ClientService: Send + Sync {
    async fn say(&self, content: String) -> String;

    async fn repetition(&self, content: String);
}

#[rpc_service]
pub trait ServerService: Send + Sync {
    async fn say(&self, content: String) -> String;
}
