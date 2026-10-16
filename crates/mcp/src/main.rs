//! Dev entry point; the shipped app serves the same thing via `G2 Boost.exe --mcp`.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    g2boost_mcp::serve().await
}
