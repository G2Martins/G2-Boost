// GUI subsystem in every build (debug included): otherwise Windows opens a console window next
// to the app. `--mcp` still works — Claude Desktop/Code talk to it over inherited pipes, not a console.
#![windows_subsystem = "windows"]

fn main() {
    g2_boost_lib::run()
}
