use std::io::stdout;

use clap::Parser;
use crossterm::{
    event::{EventStream, KeyCode, KeyEvent, KeyModifiers},
    terminal::SetTitle,
    ExecutableCommand,
};
use futures::StreamExt;
use itertools::Itertools;
use ratatui::{layout, widgets};
use regex::Regex;

use crate::with_tui::WithTui;

const WORDS: &str = include_str!("../../data/words.txt");

#[derive(Debug, Parser)]
#[clap(about = "Search for English words matching a regex input.")]
pub struct WordCommand {
    #[arg(short, long, help = "Launch an interactive TUI to input regexes")]
    interactive: bool,
}

impl WithTui for WordCommand {}

impl WordCommand {
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = self.tui_setup()?;
        let mut event_stream = EventStream::new();
        let mut match_engine = MatchEngine::new();
        let mut current_page = 0;
        loop {
            stdout().execute(SetTitle(format!(
                "{} - {}",
                std::env::args().join(" "),
                match_engine.pattern,
            )))?;
            terminal.draw(|f| {
                let chunks = layout::Layout::default()
                    .direction(layout::Direction::Vertical)
                    .margin(2)
                    .constraints(
                        [layout::Constraint::Length(3), layout::Constraint::Min(0)].as_ref(),
                    )
                    .split(f.size());
                let input_widget =
                    widgets::Paragraph::new(format!(" > {}", match_engine.pattern.clone()))
                        .block(widgets::Block::default().borders(widgets::Borders::ALL))
                        .wrap(widgets::Wrap { trim: true });
                // TODO: nicer table formatting, ellipsis
                let matches = match_engine
                    .matches()
                    .unwrap_or_else(|_| vec!["Error parsing regex!"]);
                let column_spacing = 2;
                let len_longest_match = matches.iter().map(|s| s.len()).max().unwrap_or(0);
                let n_columns_wanted = matches.len() / chunks[1].width as usize;
                let n_columns_available =
                    chunks[1].width as usize / (len_longest_match + column_spacing);
                let n_columns = n_columns_wanted.min(n_columns_available).max(1);
                let n_rows = chunks[1].height as usize;
                let column_widths =
                    vec![layout::Constraint::Length(len_longest_match as u16); n_columns];
                let n_words_visible = n_rows * n_columns;
                let start_at = current_page * n_words_visible;
                // TODO: don't allow paging past end
                let table_entries: Vec<widgets::Row> = transpose(
                    matches
                        .iter()
                        .skip(start_at)
                        .chunks(n_rows)
                        .into_iter()
                        .map(|chunk| chunk.collect())
                        .collect(),
                )
                .iter()
                .map(|row| {
                    widgets::Row::new(row.into_iter().map(|s| widgets::Cell::from(s.to_string())))
                })
                .collect();
                let matches_table = widgets::Table::new(table_entries)
                    .widths(column_widths.as_slice())
                    //.column_spacing(column_spacing as u16)
                    .block(
                        widgets::Block::default()
                            .title(format!("Matches ({} total)", matches.len()))
                            .borders(widgets::Borders::ALL),
                    );
                // TODO: help widget
                f.render_widget(input_widget, chunks[0]);
                f.render_widget(matches_table, chunks[1]);
            })?;
            match event_stream.next().await {
                Some(Ok(event)) => match event {
                    crossterm::event::Event::Key(key) => match key {
                        KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                        } => break,
                        KeyEvent {
                            code: KeyCode::Char('u'),
                            modifiers: KeyModifiers::CONTROL,
                        } => current_page = current_page.saturating_sub(1),
                        KeyEvent {
                            code: KeyCode::Char('d'),
                            modifiers: KeyModifiers::CONTROL,
                        } => current_page = current_page.saturating_add(1),
                        KeyEvent {
                            code: KeyCode::Char(c),
                            ..
                        } => {
                            match_engine.pattern.push(c);
                            current_page = 0;
                        }
                        KeyEvent {
                            code: KeyCode::Backspace,
                            ..
                        } => {
                            match_engine.pattern.pop();
                            current_page = 0;
                        }
                        KeyEvent {
                            code: KeyCode::Esc, ..
                        } => break,
                        _ => {}
                    },
                    _ => {}
                },
                Some(Err(e)) => panic!("error reading input: {}", e),
                None => break,
            }
        }
        self.tui_shutdown(&mut terminal)?;
        Ok(())
    }
}

#[derive(Debug)]
struct MatchEngine {
    pattern: String,
}

impl MatchEngine {
    fn new() -> Self {
        Self {
            pattern: String::new(),
        }
    }

    fn matches(&self) -> Result<Vec<&str>, regex::Error> {
        let result: Vec<&str> = Regex::new(&format!(r"(?m)^{}$", self.pattern))?
            .find_iter(&WORDS)
            .map(|match_| match_.as_str())
            .collect();
        if result.len() == 1 && result[0] == "" {
            Ok(vec![])
        } else {
            Ok(result)
        }
    }
}

fn transpose<T>(v: Vec<Vec<T>>) -> Vec<Vec<T>>
where
    T: Clone,
{
    v.iter().fold(vec![], |acc, row| {
        row.iter().enumerate().fold(acc, |mut acc, (i, cell)| {
            if acc.len() <= i {
                acc.push(vec![]);
            }
            acc[i].push(cell.clone());
            acc
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpose() {
        assert_eq!(
            transpose(vec![vec![1, 2, 3]]),
            vec![vec![1], vec![2], vec![3]]
        );
        assert_eq!(
            transpose(vec![vec![1, 2, 3], vec![4, 5, 6]]),
            vec![vec![1, 4], vec![2, 5], vec![3, 6]]
        );
        assert_eq!(
            transpose(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]]),
            vec![vec![1, 4, 7], vec![2, 5, 8], vec![3, 6, 9]]
        );
    }
}
