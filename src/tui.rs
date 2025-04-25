use super::app;
use color_eyre::Result;
use ratatui::{
    backend::{
        Backend,
        CrosstermBackend,
    },
    crossterm::{
        terminal::{
            disable_raw_mode,
            enable_raw_mode,
            EnterAlternateScreen,
            LeaveAlternateScreen,
        },
        ExecutableCommand,
    },
    Terminal,
};
use std::{
    io::stdout,
    panic,
};

fn init_terminal() -> color_eyre::Result<Terminal<impl Backend>> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    Ok(terminal)
}

fn restore_terminal() -> color_eyre::Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        stdout().execute(LeaveAlternateScreen).unwrap();
        disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}

pub async fn run(args: crate::args::Args) -> Result<()> { // Accept parsed args
    install_panic_hook();

    let mut terminal = init_terminal()?;
    defer! {
        if let Err(err) = restore_terminal() {
            error!("Error restoring terminal: {err}");
        }
    }

    // 1. Load config from file
    let mut config = crate::config::Config::load().unwrap_or_else(|e| {
        error!("Failed to load config, using defaults: {}", e);
        crate::config::Config::default()
    });

    // 2. Update config from args (saves if changed)
    if let Err(e) = config.update_from_args(&args) {
         error!("Failed to update/save config from args: {}", e);
         // Decide if this is fatal or if we should continue with potentially stale config
    }

    // 3. Initialize the model with the final config
    let mut model = app::Model::new(config); // Pass the config

    while model.running_state != app::RunningState::Done {
        // Render the current view
        terminal.draw(|f| app::view(&mut model, f))?;

        // Handle events and map to a Message
        let mut current_msg = app::handle_event(&model)?;

        // Process updates as long as they return a non-None message
        while current_msg.is_some() {
            current_msg = app::update(&mut model, current_msg.unwrap());
        }
    }

    Ok(())
}
