use xy_rpc::{formats::JsonFormat, rpc_service};

pub const PORT: u16 = 30003;
pub type FormatType = JsonFormat;

#[rpc_service]
pub trait ExampleService {
    async fn hello(&self, content: String, param2: u32) -> String;
}
