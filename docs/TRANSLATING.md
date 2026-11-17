# Translation policy

Repository documentation, identifiers, and GitHub artifacts use English. English is the initial product language. Additional languages are accepted with reviewed catalogues; no French/Japanese catalogue or localization runtime exists yet.

When the UI is introduced, externalize user-facing strings behind stable keys. Choose the localization library with the UI toolkit. Keep variables and plural/select rules structured, avoid sentence concatenation, and test long labels and screen-reader names.

Add catalogue/key parity checks when there are multiple shipped languages. Document fallback and missing-key behavior. Translations must not change protocol enum values, units, server field names, stored identifiers, or transmitted frequency representation. Display formatting may be localized only while preserving precise unambiguous frequency entry.

Aurora's localization discipline informs this policy; its catalogues, language promises, font assets, and `aurora_i18n` implementation are not part of this repository.
