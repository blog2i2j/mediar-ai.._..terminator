// seperate test
use tracing::Level;
use terminator::{platforms, AutomationError};

#[tokio::test]
async fn test_open_app() -> Result<(), AutomationError> {
    tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::DEBUG)
        .init();

    let engine = platforms::create_engine(false, false)?;

    let applications = vec![
        "notepad",
        "calculator",
        "explorer",
        "cmd",
        "neovim",
        "node.js", 
        "firefox",
        "firefox private",
        "microsoft edge",
        "edge",
        "microsoft paint",
        "paint",
        "arc",
        "telegram",
        "cursor",
        "claude",
        "proton vpn",
    ];


    for app in applications {

        let opened_app = engine.open_application(app)?;
        assert!(opened_app.name().is_some(), "failed to open: {}, it should've an ui element", app);

        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    Ok(())
}

