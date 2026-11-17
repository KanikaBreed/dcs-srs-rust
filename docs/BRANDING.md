# Interface and branding

Working product name: **DCS SRS Rust**. Repository: `dcs-srs-rust`. This is an independent compatible client. Do not reuse Aurora's wolf, palette identity, application screenshots, or SRS/Eagle Dynamics logos as our own branding. No logo or icon pack has been bundled.

## TOC console priorities

1. The selected transmit radio, frequency, modulation, and PTT state are always legible.
2. Receiving radios can be distinguished while several nets are active.
3. Disconnection, permission failure, muted output, and unavailable microphone are explicit.
4. Touch PTT is large, reachable, and reliably releases on cancellation/interruption.
5. Settings never obscure whether the microphone is transmitting.

## Design system to implement

Use a compact desktop console and responsive phone/tablet layouts. Establish named tokens for surfaces, text, accent, TX, RX, warning, and error, plus shared radio cards and controls. Colors supplement text/icons, never replace them. Avoid decorative animation or continuous redraw that wastes mobile power.

Honor system text scaling, reduced motion, keyboard focus, screen readers, safe areas, and touch target guidance. Frequency input must remain precise and unambiguous across display locales. A control's label and behavior stay consistent on every screen.

Show local TX activity honestly; do not present it as confirmed remote reception when the protocol has no such acknowledgement. Distinguish reconnecting, authenticated, and ready-to-transmit states.

## Review

Capture actual implemented screens with synthetic server names and callsigns. Test narrow phones, tablets, desktop scaling, long labels, and simultaneous RX states. Compare before/after for UI changes. The desktop preview uses Slint Fluent Dark widgets, a named Theme token global, a fixed visible transmit/status area and scrolling radio editors. [DESKTOP.md](DESKTOP.md) provides synthetic RX/TX screenshot commands. Mobile layouts, full screen-reader validation and broader scaling checks remain pending.
