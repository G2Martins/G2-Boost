//! Dev entry point; the shipped app serves the same thing via `G2 Boost.exe --mcp`.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // AMD (ADLX) calls run in a helper child of the current exe; answer it here too.
    if g2boost_core::adlx::helper_if_requested() {
        return Ok(());
    }
    g2boost_mcp::serve().await
}
