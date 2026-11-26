//! Name des Programms, das gerade im Vordergrund eines Terminals läuft (zsh, nvim, htop …).
//! Nur über `/proc`, ohne libc: `/proc/<shell>/stat` nennt die Vordergrund-Prozessgruppe
//! (`tpgid`) des Terminals, `/proc/<tpgid>/comm` deren Namen.

use std::fs;

/// Feld 8 (`tpgid`) aus dem Inhalt von `/proc/<pid>/stat`.
/// Der Programmname in Klammern darf Leerzeichen und Klammern enthalten – daher ab der letzten `)`.
pub fn parse_tpgid(stat: &str) -> Option<i32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // nach dem Namen: state ppid pgrp session tty_nr tpgid …
    rest.split_whitespace().nth(5)?.parse().ok()
}

/// Kurzname eines Prozesses (`/proc/<pid>/comm`, max. 15 Zeichen).
pub fn comm(pid: i32) -> Option<String> {
    let name = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Vordergrund-Programm des Terminals, dessen Shell `shell_pid` ist.
pub fn foreground_name(shell_pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?;
    match parse_tpgid(&stat) {
        Some(tpgid) if tpgid > 0 => comm(tpgid).or_else(|| comm(shell_pid as i32)),
        _ => comm(shell_pid as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpgid_from_stat() {
        let stat = "4242 (zsh) S 4200 4242 4242 34817 4300 4194304 1203 0 0 0";
        assert_eq!(parse_tpgid(stat), Some(4300));
        // Name mit Leerzeichen und Klammern
        let stat = "77 (my (odd) prog) R 1 77 77 34818 -1 0";
        assert_eq!(parse_tpgid(stat), Some(-1));
        assert_eq!(parse_tpgid("kaputt"), None);
    }

    #[test]
    fn own_process() {
        let me = std::process::id();
        let name = comm(me as i32).expect("eigener Prozess hat einen Namen");
        assert!(!name.is_empty());
        // Ohne Terminal (tpgid -1) fällt es auf den eigenen Namen zurück
        assert!(foreground_name(me).is_some());
        assert_eq!(foreground_name(u32::MAX), None);
    }
}
