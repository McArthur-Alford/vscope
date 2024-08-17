use crate::tui;
use std::io;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use anyhow::anyhow;
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
use ratatui::layout::{Direction, Layout};
use ratatui::prelude::{Color, Style, Modifier, Constraint, StatefulWidget};
use ratatui::symbols::block;
use ratatui::widgets::{Borders, HighlightSpacing, List, ListDirection, ListItem, ListState};
use vs_core::{connect_to_daemon, Message, TrackArgs};
use crate::app_interactive::AppEvent::Run;

#[derive(Debug, Default)]
struct StatefulList {
    state: ListState,
    items: Vec<PathBuf>,
}
#[derive(Debug, Default)]
pub struct AppInteractive {
    items: StatefulList,
    status: AppEvent,
}

#[derive(Debug, Default)]
enum AppEvent {
    #[default]
    Run,
    Quit,
    Save,
}

impl AppInteractive {
    /// runs the application's main loop until the user quits
    pub async fn run(&mut self, terminal: &mut tui::Tui) -> anyhow::Result<Option<String>> {
        let mut connection = connect_to_daemon().await?;
        let message = Message::Get(10);
        let paths = match connection.communicate(message).await? {
            Message::Paths(paths) => { paths },
            invalid => { panic!("Received incorrect response from daemon: {:?}", invalid) }
        };
        
        if paths.len() != 10 {
            panic!("Received less than 10 paths: ({}) {}", 
                paths.len(),
                   paths.iter().map(|path| path.display().to_string()).collect::<Vec<String>>().join(", "));
        }

        self.items = StatefulList::with_items(paths);

        while matches!(self.status, Run) {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events()?;
        }

        match self.status {
            AppEvent::Save => {
                let test = self.items
                    .state
                    .selected()
                    .map(|selected| self.items.items.get(selected)
                        .unwrap_or(&PathBuf::from("magic")).display().to_string());
                Ok(test)
            }
            AppEvent::Quit => {
                Ok(Some("Test".to_string()))
            }
            _ => {
                Err(anyhow!("Invalid status"))
            }
        }
    }

    fn render_frame(&mut self, frame: &mut Frame) {
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
            KeyCode::Up => self.items.previous(),
            KeyCode::Down => self.items.next(),
            KeyCode::Enter => self.save(),
            _ => {}
        }
    }

    fn save(&mut self) {
        self.status = AppEvent::Save;
    }

    fn exit(&mut self) {
        self.status = AppEvent::Quit;
    }
}

impl Widget for &mut AppInteractive {
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

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(67),
            ].as_ref())
            .split(area);

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
            self.items.state.selected().unwrap_or(0).to_string().yellow(),
        ])]);

        let list_area = Layout::default()
            .horizontal_margin(2)
            .vertical_margin(1)
            .constraints([
                Constraint::Percentage(100),
            ].as_ref())
            .split(chunks[0]);

        // Iterate through all elements in the `items` and stylize them.
        let items: Vec<ListItem> = self
            .items
            .items
            .iter()
            .cloned()
            .map(|path| ListItem::new(path.display().to_string()))
            .collect();

        // Create a List from all list items and highlight the currently selected one
        let list = List::new(items)
            .block(Block::default().title("Paths"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED)
            )
            .highlight_symbol(">")
            .repeat_highlight_symbol(true)
            .highlight_spacing(HighlightSpacing::Always);

        let block2 = Block::bordered().borders(Borders::RIGHT);

        block2.render(chunks[0], buf);

        StatefulWidget::render(list, list_area[0], buf, &mut self.items.state);

        Paragraph::new(counter_text)
            .centered()
            .block(block)
            .render(area, buf);
    }
}


impl StatefulList {
    fn with_items(items: Vec<PathBuf>) -> StatefulList {
        StatefulList {
            state: ListState::default(),
            items: items.iter().cloned().collect(),
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0
        };
        self.state.select(Some(i));
    }

    fn unselect(&mut self) {
        let offset = self.state.offset();
        self.state.select(None);
        *self.state.offset_mut() = offset;
    }
}
