TOML Sort
==========

Simple tool for sorting toml files via cli or ci. It was made with `Cargo.toml` files in mind but is likely useful elsewhere.

## Example

Look no further. This repository itself uses toml sort. Check out the `toml-sort.toml` file right here in the repository.

## Categories

One major motivator for developing this tool is that it will sort your dependencies lexicographically while still respecting your commented in section headings.

```toml
[dependencies]
# Common ones
clap = "..."
serde = "..."

# Private things
a-secret-thing = "..."
other-unpublished-stuff = "..."
private-crate = "..."
```
