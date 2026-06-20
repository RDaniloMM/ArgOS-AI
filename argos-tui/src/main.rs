use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    argos_tui::runtime::run(argos_tui::runtime::RunOptions::from_env()).await
}
