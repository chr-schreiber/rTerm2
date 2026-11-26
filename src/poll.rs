//! Zeitplan für das Abfragen der Vordergrund-Programme – nach dem Vorbild von iTerm2
//! (`iTermProcessCache` + `iTermRateLimitedUpdate`):
//!
//! - kein fester Takt: abgefragt wird nur nach einem Anlass (Ausgabe, Tastendruck, Pane-Wechsel)
//! - Rate-Limit: höchstens alle 0,5 s, wenn ein rterm2-Fenster den Fokus hat, sonst alle 5 s;
//!   die erste Anfrage läuft sofort, weitere im Intervall werden zu einer am Intervallende
//! - Nachfassen ohne Rate-Limit nach Enter/Ctrl+C/Ctrl+Z (iTerm2 bekommt dafür fork/exec-Meldungen
//!   vom Kernel; unter Linux gibt es das ohne Root nicht)
//! - nichts geplant → die Eventloop schläft, bis wieder etwas passiert
//!
//! Ohne Abhängigkeiten, damit unit-testbar.

use std::time::{Duration, Instant};

pub const INTERVAL_FOCUSED: Duration = Duration::from_millis(500);
pub const INTERVAL_BACKGROUND: Duration = Duration::from_secs(5);
/// Nachfassen nach Enter & Co.: das neue Programm ist oft erst Millisekunden später im Vordergrund.
pub const FOLLOW_UPS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(2),
];

#[derive(Default)]
pub struct PollSchedule {
    last_run: Option<Instant>,
    due: Vec<Instant>,
}

impl PollSchedule {
    /// Anlass mit Rate-Limit (z. B. Ausgabe): sofort, falls die letzte Abfrage lange genug her ist,
    /// sonst am Intervallende. Ist dann ohnehin schon eine Abfrage geplant, deckt die das ab.
    pub fn request(&mut self, now: Instant, interval: Duration) {
        let at = match self.last_run {
            Some(last) if now < last + interval => last + interval,
            _ => now,
        };
        if !self.due.iter().any(|t| *t <= at) {
            self.due.push(at);
        }
    }

    /// Abfrage zu einem festen Zeitpunkt, ohne Rate-Limit (Nachfassen, Pane-Wechsel).
    pub fn request_at(&mut self, at: Instant) {
        if !self.due.contains(&at) {
            self.due.push(at);
        }
    }

    /// Nach Enter & Co.: jetzt und mehrmals kurz danach nachsehen.
    pub fn follow_up(&mut self, now: Instant) {
        self.request_at(now);
        for d in FOLLOW_UPS {
            self.request_at(now + d);
        }
    }

    /// Ist eine Abfrage fällig? Entfernt alle fälligen Termine (mehrere fällige = eine Abfrage).
    pub fn take_due(&mut self, now: Instant) -> bool {
        let before = self.due.len();
        self.due.retain(|t| *t > now);
        let run = self.due.len() != before;
        if run {
            self.last_run = Some(now);
        }
        run
    }

    /// Nächster Termin – bis dahin darf die Eventloop schlafen. `None` = gar nicht aufwachen.
    pub fn next(&self) -> Option<Instant> {
        self.due.iter().min().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn idle_means_no_wakeups() {
        let s = PollSchedule::default();
        assert_eq!(s.next(), None);
    }

    #[test]
    fn first_request_runs_immediately_then_rate_limited() {
        let t0 = Instant::now();
        let mut s = PollSchedule::default();
        s.request(t0, INTERVAL_FOCUSED);
        assert_eq!(s.next(), Some(t0));
        assert!(s.take_due(t0));
        assert_eq!(s.next(), None);

        // Ausgabe-Schwall innerhalb des Intervalls → genau eine Abfrage am Intervallende
        for i in 1..100 {
            s.request(t0 + i * MS, INTERVAL_FOCUSED);
        }
        assert_eq!(s.due.len(), 1);
        assert_eq!(s.next(), Some(t0 + INTERVAL_FOCUSED));
        assert!(!s.take_due(t0 + 499 * MS));
        assert!(s.take_due(t0 + INTERVAL_FOCUSED));
        assert_eq!(s.next(), None);

        // lange Pause → wieder sofort
        let later = t0 + 10 * INTERVAL_FOCUSED;
        s.request(later, INTERVAL_FOCUSED);
        assert_eq!(s.next(), Some(later));
    }

    #[test]
    fn background_interval_is_longer() {
        let t0 = Instant::now();
        let mut s = PollSchedule::default();
        s.request(t0, INTERVAL_BACKGROUND);
        assert!(s.take_due(t0));
        s.request(t0 + MS, INTERVAL_BACKGROUND);
        assert_eq!(s.next(), Some(t0 + INTERVAL_BACKGROUND));
    }

    #[test]
    fn follow_ups_ignore_rate_limit() {
        let t0 = Instant::now();
        let mut s = PollSchedule::default();
        s.request(t0, INTERVAL_FOCUSED);
        assert!(s.take_due(t0));
        // Enter 10 ms später: sofort + 100 ms + 500 ms + 2 s, obwohl das Intervall noch läuft
        let t1 = t0 + 10 * MS;
        s.follow_up(t1);
        assert_eq!(s.next(), Some(t1));
        assert!(s.take_due(t1));
        assert_eq!(s.next(), Some(t1 + 100 * MS));
        assert!(s.take_due(t1 + 100 * MS));
        assert!(s.take_due(t1 + 500 * MS));
        assert!(s.take_due(t1 + 2000 * MS));
        assert_eq!(s.next(), None);
    }

    #[test]
    fn rate_limited_request_reuses_earlier_follow_up() {
        let t0 = Instant::now();
        let mut s = PollSchedule::default();
        s.follow_up(t0);
        assert!(s.take_due(t0));
        // Ausgabe kurz danach: die Nachfass-Abfrage bei +100 ms liegt vor dem Intervallende → reicht
        s.request(t0 + 20 * MS, INTERVAL_FOCUSED);
        assert_eq!(s.due.len(), 3);
    }
}
