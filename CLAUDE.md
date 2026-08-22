# media-query-parse

Rust-Crate: eigenständiger Parser für die CSS-Media-Queries-Grammatik.
Konzept & Herkunft: `README.md`. Umsetzungsplan: `plan/`.

Projektname (Repo) und voraussichtlicher crates.io-Paketname sind hier
identisch, `media-query-parse`. Vor Veröffentlichung erneut prüfen.

Schwesterprojekt von [`html-conform`](../html-conform) (künftiger
Baustein für dessen `w:media-query`/`w:source-size-list`-Datatype-
Implementierung, Phase 05c dort). Steht aber für sich: generisch, kein
HTML-Bezug.

## Architektur (Arbeitstitel, siehe `plan/` für Details)

```
Media-Query-String → Tokenizer (CSS-Syntax-Level-3-Grundlage)
                    → Parser (Media-Queries-Level-4/5-Grammatik)
                    → strukturierter AST (Medientyp, Features, Bedingungen)
```

Reine Syntaxprüfung/-Struktur — dieses Crate wertet **nicht** aus, ob
eine Query zu einem echten Gerät/Viewport passt (kein "matches"-Konzept),
nur ob sie syntaktisch gültig ist und wie sie aufgebaut ist.

## Arbeitsweise

- Aktueller Stand & nächster Schritt: `plan/00-STATUS.md`.
- Phasenpläne mit Schritten/Exit-Kriterien: `plan/0N-*.md`. Vor größeren
  Änderungen die passende Phase lesen, nicht am Plan vorbei arbeiten.
- Getroffene Entscheidungen: `plan/DECISIONS.md` — dort nachschlagen,
  bevor offene Fragen neu aufgerollt werden.

## Feste Regeln

- Lizenz: **MIT**, von Anfang an.
- Normative Grundlage: [CSS Syntax Module Level 3](https://www.w3.org/TR/css-syntax-3/)
  für die Tokenisierung, [Media Queries Level 4](https://www.w3.org/TR/mediaqueries-4/)
  für die Grammatik — das schließt die Range-Syntax (`<mf-range>`,
  `(400px <= width <= 700px)`) mit ein, die bereits vollständig in
  Level 4 normativ definiert ist, nicht erst in Level 5 (siehe
  `plan/04-range-syntax.md`, `plan/DECISIONS.md`). Level 5 ist nur für
  tatsächlich darüber hinausgehende, dort neu hinzukommende
  Erweiterungen relevant, falls eine spätere Phase das braucht. Bei
  Unklarheiten in der Spec selbst nachschlagen, nicht aus anderen
  Implementierungen raten.
- Kein HTML-Bezug im Kern — Instanzstrings kommen als reiner `&str` rein,
  kein Parser für ein Wirtsformat.
- Kein `unsafe` ohne expliziten Grund und Kommentar.

## Definition of Done

Siehe "Exit-Kriterien" in der jeweiligen `plan/0N-*.md`-Datei — nicht
global definiert, sondern pro Phase.
