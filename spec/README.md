# `.ogp` file format specification

Licensed under [CC BY 4.0](LICENSE) — anyone may implement the format; copies of this spec must credit
"OG Paper, by OmegaGiven and contributors".

The formal spec (v1) will be written during Phase 1. Until then, the design lives in
[`../docs/DESIGN.md`](../docs/DESIGN.md#data-model-and-file-format):

- [Target v1 schema](../docs/DESIGN.md#schema-v1-draft) and [longevity guarantees](../docs/DESIGN.md#longevity-guarantees)
- [`.ogp` format 0.1](../docs/DESIGN.md#implemented-today-ogp-format-01): what the app writes today (`meta` + `objects`)
- [Time stamps and the timeline](../docs/DESIGN.md#time-stamps-and-the-timeline): the stroke event log
- [Bookmarks](../docs/DESIGN.md#bookmarks)
- [`.ogpt` snapshot v1](../docs/DESIGN.md#web-offline-copies-ogpt-snapshot-v1): the web app's offline copies (byte layout)

Every `.ogp` file also explains itself: its `meta` table has a `README` row describing the format.
