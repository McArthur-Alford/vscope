use crate::app_interactive::AppEvent::Run;
use crate::{tui_helper, SearchArgs};
use anyhow::anyhow;
use ratatui::crossterm::style::style;
use ratatui::layout::{Direction, Layout};
use ratatui::prelude::{Constraint, Modifier, Span, StatefulWidget};
use ratatui::text::Text;
use ratatui::widgets::{Borders, HighlightSpacing, List, ListItem, ListState};
use ratatui::{
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Alignment, Rect},
    style::Stylize,
    symbols::border,
    text,
    text::Line,
    widgets::{
        block::{Position, Title},
        Block, Paragraph, Widget,
    },
    Frame,
};
use serde_json::ser::CharEscape::CarriageReturn;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::{env, fs, io};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::syntax_definition::ContextReference::File;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use tui::style::Color;
use vs_core::{connect_to_daemon, Connection, Message};

#[derive(Debug, Default)]
struct StatefulList {
    state: ListState,
    items: Vec<PathBuf>,
    preview: Option<String>,
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
    pub async fn run(
        &mut self,
        terminal: &mut tui_helper::Tui,
        connection: &mut Connection,
        args: &SearchArgs,
    ) -> anyhow::Result<Option<String>> {
        let message = Message::Get(200);
        let paths = match connection.communicate(message).await? {
            Message::Paths(paths) => paths,
            invalid => {
                panic!("Received incorrect response from daemon: {:?}", invalid)
            }
        };

        let pwd = env::current_dir()?;
        let paths = paths
            .into_iter()
            .filter(|p| p.starts_with(pwd.clone()))
            .filter_map(|p| {
                let stripped = p.strip_prefix(pwd.clone()).map(|p| p.to_path_buf());
                stripped.ok()
            })
            .map(|p| {
                let mut p2 = PathBuf::new();
                p2.push(".");
                p2.push(p);
                p2
            })
            .filter(|p| {
                p.is_file()
                    || args.directories && p.is_dir()
                    || args.args.symlinks && p.is_symlink()
            })
            .collect::<Vec<PathBuf>>();

        self.items = StatefulList::with_items(paths);

        while matches!(self.status, Run) {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events()?;
        }

        match self.status {
            AppEvent::Save => {
                let test = self.items.state.selected().map(|selected| {
                    self.items
                        .items
                        .get(selected)
                        .unwrap_or(&PathBuf::from("magic"))
                        .display()
                        .to_string()
                });
                Ok(test)
            }
            AppEvent::Quit => Ok(Some("".to_string())),
            _ => Err(anyhow!("Invalid status")),
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

    fn get_preview(&self, num_lines: usize) -> Vec<Line> {
        let unavailable = vec![Line::from("Preview unavailable")];

        match &self.items.preview {
            None => unavailable,
            Some(content) => {
                match self.items.items.get(self.items.state.selected().unwrap()) {
                    Some(path) if path.is_file() => {
                        let extension = match path.extension() {
                            Some(os_ext) => match os_ext.to_str() {
                                Some(ext) => ext,
                                None => {
                                    return content.split('\n').map(|line| {
                                        Line::from(line)
                                    }).collect();
                                }
                            },
                            _ => {
                                return content.split('\n').map(|line| {
                                    Line::from(line)
                                }).collect();
                            }
                        };

                        let ps = SyntaxSet::load_defaults_newlines();
                        let ts = ThemeSet::load_defaults();

                        let syntax = ps.find_syntax_by_extension(extension);

                        match syntax {
                            Some(syntax) => {
                                let mut h =
                                    HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);
                                LinesWithEndings::from(&content)
                                    .take(num_lines)
                                    .map(|line| {
                                        // LinesWithEndings enables use of newlines mode
                                        let line_spans = h.highlight_line(line, &ps).unwrap();
                                        Line::from(
                                            line_spans
                                                .iter()
                                                .map(|(ref style, text)| {
                                                    Span::styled(
                                                        text.to_string(),
                                                        ratatui::prelude::Style::new().fg(
                                                            ratatui::prelude::Color::Rgb(
                                                                style.foreground.r,
                                                                style.foreground.b,
                                                                style.foreground.b,
                                                            ),
                                                        ),
                                                    )
                                                })
                                                .collect::<Vec<Span>>(),
                                        )
                                    })
                                    .collect()
                            }
                            None => {
                                content.split('\n').map(|line| {
                                    Line::from(line)
                                }).collect()
                            },
                        }
                    }
                    _ => {
                        content.split('\n').map(|line| {
                            Line::from(line)
                        }).collect()
                    },
                }
            }
        }
    }
}

impl Widget for &mut AppInteractive {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let app_title = Title::from(" Vibe Search ".bold());
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
            .constraints(
                [
                    Constraint::Percentage(33),
                    Constraint::Length(1),
                    Constraint::Percentage(67),
                ]
                .as_ref(),
            )
            .split(area);

        let block = Block::bordered()
            .title(app_title.alignment(Alignment::Center))
            .title(
                instructions
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .border_set(border::THICK);

        let list_area = Layout::default()
            .horizontal_margin(2)
            .vertical_margin(1)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(chunks[0]);

        let border_area = Layout::default()
            .vertical_margin(1)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(chunks[1]);

        let preview_area = Layout::default()
            .horizontal_margin(2)
            .vertical_margin(1)
            .constraints([Constraint::Percentage(100)].as_ref())
            .split(chunks[2]);

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
                ratatui::prelude::Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED),
            )
            .highlight_symbol(">")
            .repeat_highlight_symbol(true)
            .highlight_spacing(HighlightSpacing::Always);

        let border = Block::bordered().borders(Borders::RIGHT);

        block.render(area, buf);

        Paragraph::new(Text::from(
            self.get_preview(preview_area[0].height as usize),
        ))
        .render(preview_area[0], buf);

        border.render(border_area[0], buf);

        StatefulWidget::render(list, list_area[0], buf, &mut self.items.state);
    }
}

impl StatefulList {
    fn with_items(items: Vec<PathBuf>) -> StatefulList {
        StatefulList {
            state: ListState::default(),
            items: items.iter().cloned().collect(),
            preview: None,
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

        self.update_path_contents();
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
            None => 0,
        };
        self.state.select(Some(i));

        self.update_path_contents();
    }

    fn update_path_contents(&mut self) {
        let i = self.state.selected();

        if matches!(i, None) {
            panic!("No i value found");
        }

        let selected = self.items.get(i.unwrap());

        self.preview = match selected {
            Some(path) if path.is_file() => {
                fs::read_to_string(path).ok()
            },
            Some(path) if path.is_dir() => {                
                let entries = fs::read_dir(path);
                
                match entries {
                    Ok(entries) => {
                        let mut paths = Vec::new();
                        for entry in entries {
                            match entry {
                                Ok(entry) => {
                                    let path = entry.path();
                                    if let Some(path_str) = path.to_str() {
                                        paths.push(path_str.to_string());
                                    }
                                }
                                Err(_) => {}
                            }
                        }

                        Some(paths.join("\n"))
                    }
                    Err(_) => {
                        None
                    }
                }
                
            },
            _ => None
        }
    }

    fn unselect(&mut self) {
        let offset = self.state.offset();
        self.state.select(None);
        *self.state.offset_mut() = offset;
    }
}
