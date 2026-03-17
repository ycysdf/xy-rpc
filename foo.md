```shell
cargo test --features rt_tokio,format_message_pack
cargo test --features rt_tokio_without_send_sync,format_message_pack
cargo test --features rt_tokio,duplex,format_message_pack
cargo test --features axum,format_message_pack
cargo test --features rt_tokio,stream,format_message_pack
cargo test --features rt_tokio,format_message_pack
cargo test --features rt_compio,format_message_pack
```