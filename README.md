This is a **Hyper.Video Browser Client Simulator** - a Rust-based testing framework that simulates multiple browser clients connecting to Hyper.Video sessions. It automates browser interactions using Chromium via the `chromiumoxide` library to test real-time video conferencing functionality at scale.

The project is a Cargo workspace with multiple binaries for different use cases:
- **client-simulator** (main TUI): Interactive terminal UI for manual testing
- **client-simulator-http**: HTTP/WebSocket server for remote control
- **client-simulator-orchestrator**: Batch orchestration of multiple simulated clients
- **client-simulator-stats-gatherer**: Analytics collection from ClickHouse
