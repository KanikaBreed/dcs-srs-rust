# Translation policy

Repository documentation, identifiers, and GitHub artifacts use English. English is the initial product language. Additional languages are accepted with reviewed catalogues; no additional language catalogue is shipped yet.

When the UI is introduced, externalize user-facing strings behind stable keys. Choose the localization library with the UI toolkit. Keep variables and plural/select rules structured, avoid sentence concatenation, and test long labels and screen-reader names.

Add catalogue/key parity checks when there are multiple shipped languages. Document fallback and missing-key behavior. Translations must not change protocol enum values, units, server field names, stored identifiers, or transmitted frequency representation. Display formatting may be localized only while preserving precise unambiguous frequency entry.

Aurora's localization discipline informs this policy; its catalogues, language promises, font assets, and `aurora_i18n` implementation are not part of this repository.

The desktop UI uses Slint `@tr` with stable contexts and named properties in [strings.slint](../crates/srs-desktop/ui/strings.slint). Slint's bundled-translation workflow is the chosen route for future reviewed catalogues. Until a catalogue is shipped, the English source text is the fallback; there is no external MO-file loading or language selector. Error codes map to those strings, without copying server/credential text into messages. Protocol labels AM/FM and numeric MHz values remain unlocalized. New languages must include key/context parity and long-label/screen-reader checks before shipping. See [Slint translation documentation](https://docs.slint.dev/latest/docs/slint/guide/development/translations/).
