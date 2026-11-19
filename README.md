# lsdocs-transform-highlight

Part of [lsdocs](https://github.com/lsdocs), a static-site generator whose
parsers, transformers and renderers are plugins.

`crates/transform-highlight` (`lsdocs-transform-highlight`, builtin
`transform-highlight`): syntax highlighting of code blocks with syntect,
as `custom:highlight` nodes renderers write as HTML.

Its settings and behavior are described in the lsdocs documentation
(https://lsdocs.github.io, "transform-highlight").

## Known limitations

- Highlighted lines (`data-lines`) and line numbers are carried as data
  but not drawn by the `default` theme (see lsdocs-render-html).

## TODO

- [ ] Nothing open here.
