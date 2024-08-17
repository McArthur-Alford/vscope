use crate::tui;
use std::io;
use std::path::PathBuf;
use ratatui::{
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Alignment, Rect},
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{
        block::{Position, Title},
        Block, Paragraph, Widget,
    },
    Frame,
};



#[derive(Debug, Default)]
pub struct AppInteractive {
    selected_index: usize,
    paths: Vec<PathBuf>,
    exit: bool,
}

impl AppInteractive {
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut tui::Tui) -> io::Result<String> {
        while !self.exit {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events()?;
        }
        
        let ret: String = self.paths
            .get(self.selected_index)
            .unwrap_or(&PathBuf::from(""))
            .display()
            .to_string();
        
        Ok(ret)
    }

    fn render_frame(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Up => self.prev_index(),
            KeyCode::Down => self.next_index(),
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn next_index(&mut self) {
        self.selected_index = self.selected_index.saturating_add(1)
            .clamp(0, self.paths.len().saturating_sub(1));
    }

    fn prev_index(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }
}

impl Widget for &AppInteractive {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let app_title = Title::from(" Counter App Tutorial ".bold());
        let instructions = Title::from(Line::from(vec![
            " Accept ".into(),
            "<Enter>".blue().bold(),
            " Next ".into(),
            "<Down>".blue().bold(),
            " Prev ".into(),
            "<Up>".blue().bold(),
            " Quit ".into(),
            "<Q>".blue().bold(),
        ]));
        let block = Block::bordered()
            .title(app_title.alignment(Alignment::Center))
            .title(
                instructions
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .border_set(border::THICK);

        let counter_text = Text::from(vec![Line::from(vec![
            "Value: ".into(),
            self.selected_index.to_string().yellow(),
        ])]);

        Paragraph::new(counter_text)
            .centered()
            .block(block)
            .render(area, buf);
    }
}