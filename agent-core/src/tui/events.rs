//! Event system — merges terminal input events with application events.
//!
//! Mirrors `src/ink/events/` (10 files). Provides a unified event stream
//! that combines keyboard/mouse/resize events from crossterm with
//! periodic tick events for animations.
#![allow(dead_code)]

use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};
use std::time::Duration;

/// Application events consumed by the main loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A keyboard event.
    Key(KeyEvent),
    /// Mouse event (scroll wheel, click).
    Mouse(MouseEvent),
    /// Terminal resize.
    Resize(u16, u16),
    /// Periodic tick (for animations, spinners).
    Tick,
    /// R4: Bracketed paste event.
    Paste(String),
}

/// Async event stream that polls crossterm and emits `AppEvent`s.
pub struct EventStream {
    tick_rate: Duration,
}

impl EventStream {
    pub fn new() -> Self {
        EventStream {
            tick_rate: Duration::from_millis(100),
        }
    }

    pub fn with_tick_rate(tick_rate: Duration) -> Self {
        EventStream { tick_rate }
    }

    /// Poll for the next event.
    ///
    /// Returns `None` if no event is available (shouldn't happen in practice
    /// since we always return Tick as a fallback).
    pub async fn next(&self) -> AppEvent {
        // Use tokio::task::spawn_blocking since crossterm::event::poll is blocking
        let tick_rate = self.tick_rate;
        tokio::task::spawn_blocking(move || {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        // Only process Press events — ignore Release/Repeat
                        // (crossterm 0.27+ sends all three on supported terminals)
                        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                            return AppEvent::Key(key);
                        }
                        Ok(Event::Resize(w, h)) => {
                            return AppEvent::Resize(w, h);
                        }
                        // R4: Bracketed paste
                        Ok(Event::Paste(text)) => {
                            return AppEvent::Paste(text);
                        }
                        Ok(Event::Mouse(mouse)) => {
                            return AppEvent::Mouse(mouse);
                        }
                        _ => {
                            // Ignore Release/Repeat/Focus events, keep polling
                            continue;
                        }
                    }
                } else {
                    return AppEvent::Tick;
                }
            }
        }).await.unwrap_or(AppEvent::Tick)
    }
}

impl Default for EventStream {
    fn default() -> Self {
        Self::new()
    }
}
