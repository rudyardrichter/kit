use std::{
    iter::{once, repeat},
    time::Duration,
};

use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{FutureExt, StreamExt};
use itertools::Itertools;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets, Terminal,
};
use tokio::{
    sync::{mpsc, watch},
    time::{self, MissedTickBehavior},
};

use crate::with_tui::WithTui;

#[derive(Clone, Copy, Debug)]
enum PomoSegment {
    Work(u64),
    ShortBreak(u64),
    LongBreak(u64),
}

impl PomoSegment {
    fn duration(&self) -> Duration {
        let minutes = match self {
            PomoSegment::Work(minutes) => minutes,
            PomoSegment::ShortBreak(minutes) => minutes,
            PomoSegment::LongBreak(minutes) => minutes,
        };
        Duration::from_secs(minutes * 60)
    }
}

impl From<&PomoSegment> for &str {
    fn from(segment: &PomoSegment) -> Self {
        match segment {
            PomoSegment::Work(_) => "Work",
            PomoSegment::ShortBreak(_) => "Short break",
            PomoSegment::LongBreak(_) => "Long break",
        }
    }
}

impl std::fmt::Display for PomoSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", Into::<&str>::into(self))
    }
}

#[derive(Debug, Parser)]
#[clap(about = "Run pomodoro timers. Press 'h' to see help for keyboard shortcuts while running.")]
pub struct Pomo {
    #[arg(
        short,
        long,
        help = "Length of work periods",
        value_name = "MINUTES",
        default_value_t = 25
    )]
    time: u64,

    #[arg(
        short,
        long,
        help = "Length of break periods",
        value_name = "MINUTES",
        default_value_t = 5
    )]
    break_: u64,

    #[arg(
        short,
        long,
        help = "Length of long break periods",
        value_name = "MINUTES",
        default_value_t = 15
    )]
    long_break: u64,

    #[arg(
        short,
        long,
        help = "Number of work periods per long break",
        value_name = "NUMBER",
        default_value_t = 3
    )]
    n_pomos: u64,
}

impl WithTui for Pomo {}

impl Pomo {
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // create iterator that goes: work, short, work, short, ..., work, long, repeat
        let segments_once = Itertools::intersperse(
            repeat(PomoSegment::Work(self.time)).take(self.n_pomos as usize),
            PomoSegment::ShortBreak(self.break_),
        )
        .chain(once(PomoSegment::LongBreak(self.long_break)));
        let segments_list = segments_once.clone().collect::<Vec<PomoSegment>>();
        let mut terminal = self.tui_setup()?;
        let mut event_stream = EventStream::new();
        let mut show_help = false;
        'outer: for (i, segment) in segments_once.cycle().enumerate() {
            let duration = segment.duration();
            let (tx_remaining, rx_remaining) = watch::channel(duration);
            let mut is_paused = false;
            let (tx_paused, rx_paused) = watch::channel(is_paused);
            let (tx_cancel, rx_cancel) = mpsc::channel(1);
            let countdown_handle = tokio::spawn(countdown(
                duration,
                tx_remaining,
                rx_paused.clone(),
                rx_cancel,
            ));
            while !countdown_handle.is_finished() {
                let remaining = rx_remaining.borrow().clone();
                display_countdown(
                    &mut terminal,
                    &segments_list,
                    i,
                    remaining,
                    duration,
                    is_paused,
                    show_help,
                )?;
                tokio::select! {
                    _ = time::sleep(Duration::from_millis(100)) => {}
                    maybe_event = event_stream.next().fuse() => {
                        match maybe_event {
                            Some(Ok(event)) => {
                                match PomoInput::try_from(event) {
                                    Ok(PomoInput::Help) => {
                                        show_help = !show_help;
                                    }
                                    Ok(PomoInput::Pause) => {
                                        is_paused = !is_paused;
                                        tx_paused.send(is_paused)?;
                                    }
                                    Ok(PomoInput::Skip) => {
                                        tx_cancel.try_send(())?;
                                        continue 'outer;
                                    }
                                    Ok(PomoInput::Quit) => {
                                        break 'outer;
                                    }
                                    Err(_) => {}
                                }
                            }
                            Some(Err(e)) => panic!("error reading input: {}", e),
                            None => break,
                        }
                    }
                }
            }
        }
        self.tui_shutdown(&mut terminal)?;
        Ok(())
    }
}

#[derive(Debug)]
enum PomoInput {
    Help,
    Pause,
    Skip,
    Quit,
}

impl TryFrom<Event> for PomoInput {
    type Error = ();

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        match event {
            Event::Key(key_event) => PomoInput::try_from(key_event),
            Event::Mouse(_) => Err(()),
            Event::Resize(_, _) => Err(()),
        }
    }
}

impl TryFrom<KeyEvent> for PomoInput {
    type Error = ();

    fn try_from(key_event: KeyEvent) -> Result<Self, Self::Error> {
        match key_event {
            KeyEvent {
                code: KeyCode::Esc, ..
            } => Ok(PomoInput::Quit),
            KeyEvent {
                code: KeyCode::Char(' '),
                ..
            } => Ok(PomoInput::Pause),
            KeyEvent {
                code: KeyCode::Char('?'),
                ..
            } => Ok(PomoInput::Help),
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
            } => Ok(PomoInput::Quit),
            KeyEvent {
                code: KeyCode::Char('h'),
                modifiers: KeyModifiers::NONE,
            } => Ok(PomoInput::Help),
            KeyEvent {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::NONE,
            } => Ok(PomoInput::Quit),
            KeyEvent {
                code: KeyCode::Char('s'),
                modifiers: KeyModifiers::NONE,
            } => Ok(PomoInput::Skip),
            _ => Err(()),
        }
    }
}

/// Given a terminal to output on, display the TUI widgets showing the current pomodoro segment and
/// the progress in the current segment as a gauge with a countdown.
fn display_countdown(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    segments_list: &Vec<PomoSegment>,
    i_segment: usize,
    remaining: Duration,
    total: Duration,
    is_paused: bool,
    show_help: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let progress_percent = (total - remaining).as_secs_f64() / total.as_secs_f64();
    let progress_show_time = format!(
        "{:02}:{:02}",
        remaining.as_secs() / 60,
        remaining.as_secs() % 60,
    );
    let progress = widgets::Gauge::default()
        .block(
            widgets::Block::default()
                .borders(widgets::Borders::ALL)
                .title(if is_paused {
                    "Progress (PAUSED)"
                } else {
                    "Progress"
                }),
        )
        .gauge_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .label(progress_show_time.as_str())
        .ratio(progress_percent);
    terminal.draw(|f| {
        let vertical_margin = f.size().height.saturating_sub(10).div_euclid(4);
        let chunks_0 = Layout::default()
            .horizontal_margin(4)
            .vertical_margin(vertical_margin)
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9),
                Constraint::Length(6),
                Constraint::Min(0),
            ])
            .split(f.size());
        let chunks_0_0 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(18), Constraint::Min(0)])
            .split(chunks_0[0]);
        let chunks_0_1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)])
            .split(chunks_0[1]);
        let n_segment_rows = chunks_0_0[0].height as usize;
        let n_segment_padding_rows = n_segment_rows.div_euclid(2).saturating_sub(1);
        let segment_rows = once("")
            .cycle()
            .take(n_segment_padding_rows)
            .chain(
                segments_list
                    .iter()
                    .cycle()
                    .map(|segment| <&PomoSegment as Into<&str>>::into(segment)),
            )
            .skip(i_segment)
            .take(n_segment_rows)
            .map(|text| widgets::Row::new(vec![widgets::Cell::from(text)]));
        let segments_table = widgets::Table::new(segment_rows.collect::<Vec<_>>())
            .highlight_style(Style::default().fg(Color::Green))
            .highlight_symbol(" > ")
            .block(
                widgets::Block::default()
                    .borders(widgets::Borders::ALL)
                    .title("Current segment"),
            )
            .widths(&[Constraint::Length(16)]);
        let mut segments_table_state = widgets::TableState::default();
        segments_table_state.select(Some(n_segment_padding_rows));
        f.render_stateful_widget(segments_table, chunks_0_0[0], &mut segments_table_state);
        f.render_widget(progress, chunks_0_0[1]);
        // TODO: help table in chunks_0[1]
        if show_help {
            let help_table = widgets::Table::new(vec![
                widgets::Row::new(vec![
                    widgets::Cell::from("h|?").style(Style::default().fg(Color::Yellow)),
                    widgets::Cell::from("Toggle this help"),
                ]),
                widgets::Row::new(vec![
                    widgets::Cell::from("q|<Esc>").style(Style::default().fg(Color::Yellow)),
                    widgets::Cell::from("Quit"),
                ]),
                widgets::Row::new(vec![
                    widgets::Cell::from("<Space>").style(Style::default().fg(Color::Yellow)),
                    widgets::Cell::from("Pause timer"),
                ]),
                widgets::Row::new(vec![
                    widgets::Cell::from("s").style(Style::default().fg(Color::Yellow)),
                    widgets::Cell::from("Skip to next segment"),
                ]),
            ])
            .widths(&[Constraint::Length(8), Constraint::Length(20)])
            .block(
                widgets::Block::default()
                    .borders(widgets::Borders::ALL)
                    .title("Help"),
            );
            f.render_widget(help_table, chunks_0_1[0]);
        }
    })?;
    Ok(())
}

/// Countdown to zero, sending the remaining time to the given transmit channel. Watches for
/// pauses on the given watch channel and returns when anything is sent on the cancel channel.
async fn countdown(
    duration: Duration,
    tx_remaining: watch::Sender<Duration>,
    mut rx_paused: watch::Receiver<bool>,
    mut rx_cancel: mpsc::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tick_rate = Duration::from_millis(100);
    let mut interval = time::interval(tick_rate);
    // Delay ticks when the countdown is paused
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut elapsed_total = Duration::ZERO;
    while elapsed_total < duration {
        tx_remaining.send(duration - elapsed_total)?;
        if let Ok(()) = rx_cancel.try_recv() {
            break;
        }
        if *rx_paused.borrow() {
            tokio::select! {
                _ = rx_cancel.recv() => {
                    break;
                }
                _ = rx_paused.changed() => {}
            }
            rx_paused.wait_for(|paused| !paused).await?;
        }
        interval.tick().await;
        elapsed_total += tick_rate;
    }
    Ok(())
}
