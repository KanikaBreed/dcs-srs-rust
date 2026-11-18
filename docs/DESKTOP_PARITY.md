# Desktop menu coverage

Reference: Ciribob and contributors' DCS SimpleRadio Standalone at `d01f540ce5e363e9ac841775c8f7e2253138cd81`, inspected from source. This is a functional inventory, not a promise of identical layout or full stock-client parity. Windows/Linux standalone TOC remains ahead of mobile and DCS integration. No inert controls stand in for unimplemented features.

Reference paths below are relative to `DCS-SR-Client/UI/ClientWindow/`. The reference's translated text keys and bindings were inspected, without copying its XAML, artwork or sound assets.

| Reference screen | Controls represented | Rust boundary / work |
|---|---|---|
| `MainWindow.xaml`, General | Microphone/output, output boost, server address, EAM password/name, connection/voice/game status, profile/unit/position, connected clients | Device/connection/callsign/EAM implemented. Asynchronous device refresh, hostname resolution and connection health implemented. Hardware boost/listening pending; game/unit/position depend on a later DCS adapter. |
| `Favourites/FavouriteServersView.xaml` | Add/remove/name/address/default/EAM favourites | Named servers and explicit load implemented; passwords intentionally excluded. Auto-joining a saved server is not implied by launch. |
| `InputSettingsControl/InputSettings.xaml` | Device rescan, PTT, ten radio selections, intercom/PTT, next/previous radio, frequency steps, guard/key/key steps, channel steps, volume steps, overlay toggles, transponder | Foreground held PTT implemented; configurable F1–F12/mouse-only foreground key and saved preferences implemented. Explicit Windows/X11/Wayland keyboard and SDL joystick buttons are implemented. Joystick modifier chords, axes/POV, other action bindings and physical input validation remain pending. Radio operations have explicit GUI controls; DCS/transponder/overlay bindings follow their adapters. |
| `ClientSettingsControl/ClientSettings.xaml`, interface/profile | Global/profile settings, create/copy/rename/delete, theme, tray/minimize, transmitter names, autoconnect prompts, updates, path/admin/input options | Named radio presets and local persistence implemented. Six-page navigation/help and radio-only native preset import/export implemented. Tray/startup/update service and upstream input-profile format remain pending. No admin requirement or updater is added by default. |
| Same, microphone/incoming/VOX | Denoise, AGC and target/max levels, VOX mode/threshold/hold | Not implemented; DSP must have deterministic signal tests and later listening evidence. VOX is a separate transmit mode, not a cosmetic checkbox. |
| Same, recording | Allow others to record, record/mixdown, format/quality, disallowed tone | Not implemented; current identity disallows recording. Consent, storage bounds and explicit capture UI precede any recorder. |
| Same, integration | DCS PTT/selection, HOTAS, expansion, rotary, start/release delay, server/EAM presets, instructor mode | Standalone TOC uses immediate release, current-state background input and finite caps for foreground/portal-edge input. DCS integration, instructor intercom and server preset format remain pending. |
| Same, effects/audio | RX/TX/start/end/intercom/key/MIDS/HQ/NATO sounds, per-model effects, clipping, background/HF/ambient gain, ten radio/intercom stereo channels | Local per-radio receive gain/mute/RX implemented. Discrete Left/Right/Both routing implemented with full-level mono fallback; continuous balance, smooth gain changes and receive-priority scoring remain pending. Effects/assets need provenance and DSP/listening checks. |
| `ClientList/ClientListWindow.xaml` | Connected clients, coalition, radio/state metadata | Bounded, sanitized standalone client list implemented; not a new source of server-hidden information. |
| `ServerSettingsWindow/ServerSettingsWindow.xaml` | Coalition/spectator, distance/LOS, IRL TX, interference/expansion/EAM, encryption/strictness, counts/names/presets, version, node limit | Read-only server policy/status implemented. The client cannot administer server settings; unsupported DSP/terrain flags are identified as such. |
| `AwacsRadioOverlayWindow/*`, `RadioOverlayWindow/*` | Radio selection/tuning/guard/key/presets, retransmit/simulcast, intercom/instructor/transponder | Standalone ten-radio editor and preset loading implemented. Separate overlays, retransmit/simulcast, instructor/intercom and transponder are later work. |
| `MainWindow.xaml`, Help | Setup, controls, troubleshooting and localization guidance | In-app standalone help/version/license and session health counters implemented. English strings stay externalized; translation catalogue and screen-reader checks remain pending. |

## Validation boundary

Automation uses generated audio, loopback/private stock SRS, process-local null audio, temporary profiles, virtual joystick, private simulated portals and private Xvfb/KWin. Physical native file dialogs, real device removal, actual keys/joysticks, headsets, listening, Wayland permission interaction, accessibility and installer/signing acceptance must be recorded separately. A completed software task does not make those human checks pass.

Connection now includes finite offline microphone/codec-loopback/stereo checks; Controls adds background input settings. See [voice validation](VOICE_VALIDATION.md) for scope and pending listening checks.

## Stock-style Linux client direction

The requested direction is familiar stock SRS look, feel and feature coverage, with usability improvements. The first layout pass follows `MainWindow.xaml` at the pinned revision: General, Controls, Favourites, Settings and Help, with direct Radio and Server/client access. General groups devices and standalone EAM connection fields, exposes working shortcuts, and shows server/local-audio/client status. Settings contains the existing connection-recovery and offline audio controls. Persistent TX/RX, selected radio, disconnect and diagnostic Stop improve visibility across pages. This remains a resizable independent UI, not a pixel-identical WPF port.

Feature work proceeds separately from appearance, using the inventory above:

1. Audio controls: master output/microphone gain, continuous channel balance and signal processing, with deterministic signal and listening validation.
2. Input parity: per-radio selection/PTT, next/previous radio and tuning actions, broader HOTAS bindings.
3. Radio workflow: compact radio window/overlay, server presets and transmitter information where authorized by server policy.
4. Optional VOX, radio effects, recording and consent, each with explicit behavior and validation.
5. DCS cockpit radio/Lua integration and its game status; standalone EAM does not establish this capability.

These items remain planned unless the inventory explicitly says implemented. No inactive stock-style controls imply features that are not present. Hardware and visual acceptance remain separate from automated checks.
