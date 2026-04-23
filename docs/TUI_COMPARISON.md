# TUI Framework Comparison

Two implementations of Portal's split-pane TUI browser, built from the same core engine on `main`.

## At a Glance

| Dimension | Ratatui (`tui/ratatui`) | FrankenTUI (`tui/ftui`) |
|-----------|------------------------|------------------------|
| Crate | `ratatui` 0.30 + `crossterm` 0.29 | `ftui` 0.3.1 (git dep) |
| Architecture | Imperative event loop | Elm/Bubbletea `Model` trait |
| TUI LOC | 754 | 904 |
| Files | 4 (`mod`, `app`, `ui`, `event`) | 3 (`mod`, `app`, `ui`) |
| Maturity | Stable, large ecosystem | Experimental (v0.3.1) |
| crates.io | Published | Git-only (partial publish) |
| Build flag | `--features tui-ratatui` | `--features tui-ftui` |
| Compiles | Yes | Yes |

## Architecture Comparison

### Ratatui: Imperative Loop

```
loop {
    terminal.draw(|frame| render(frame, &mut app))?;  // render
    if handle_events(&mut app)? { break; }              // update
}
```

- **You own the loop.** Poll events, dispatch to handler, draw. Full control.
- State is a mutable `App` struct passed around.
- Event handling and rendering are separate functions in separate files.
- Stateful widgets (`ListState`) are stored on `App` and passed mutably during render.

### FrankenTUI: Elm-Style Model

```rust
impl Model for PortalModel {
    type Message = Msg;
    fn update(&mut self, msg: Msg) -> Cmd<Msg> { /* state transition */ }
    fn view(&self, frame: &mut Frame) { /* pure render */ }
}
```

- **The runtime owns the loop.** You implement `Model`, the framework calls `update` then `view`.
- Events are converted to `Msg` via `From<Event>` — no raw event handling.
- `update` returns `Cmd<Msg>` (None, Quit, Batch, Task, etc.) for effects.
- Rendering is a pure function of state — no mutable borrows during view.

## Pros / Cons

### Ratatui

**Pros:**
- Battle-tested, actively maintained by a large community
- Extensive widget ecosystem (tui-textarea, tui-tree-widget, etc.)
- Comprehensive documentation and examples
- `ratatui::run()` handles terminal setup/cleanup/panic hooks
- Direct control over render timing and event polling

**Cons:**
- Manual state management — easy to have render/state desync bugs
- Stateful widget API requires passing `&mut State` during render (mixing concerns)
- No built-in subscription or async command model

### FrankenTUI

**Pros:**
- Clean separation: events → messages → state → view
- `Cmd` type for declarative side effects (Quit, Task, Batch, Tick)
- 80+ built-in widgets including CommandPalette, FilePicker, DragPreview
- Flex/Grid layout system with richer constraints (FitContent, FitContentBounded)
- Pane workspace system with drag-to-resize built in
- Degradation support for limited terminals

**Cons:**
- Experimental — only partially published to crates.io
- Requires git dependency (not suitable for published crates)
- Smaller community, less documentation
- API may change between versions
- 850K+ line codebase is ambitious for a 0.3 release

## Recommendation

**Ship with ratatui.** It's the safe choice — stable API, crates.io published, large ecosystem, proven in production tools (gitui, bottom, lazy-docker-tui, etc.).

**Watch ftui.** The Elm architecture is genuinely better for complex state management. Once it stabilizes and publishes to crates.io, it's worth reconsidering. The `Msg`/`Cmd` pattern eliminates entire categories of state bugs that ratatui apps are prone to.

**The core is identical.** Both branches modify only `src/tui/` and `src/lib.rs`. Switching frameworks later is a localized change.
