//! Deliberately panics inside a terminal guard for the PTY restoration test.
fn main() {
    let _ = ures::tui::with_terminal(|terminal| {
        terminal
            .draw(|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new("panic-test"), frame.area())
            })
            .map_err(|e| e.to_string())?;
        panic!("intentional terminal restoration test");
    });
}
