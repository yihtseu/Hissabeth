# Reduces `cargo about generate --format json` to the groups the About page
# shows: one per distinct license text, with the crates it covers.
[
    .licenses[]
    | {
        license: .id,
        text: .text,
        packages: [
            .used_by[].crate
            | select(.name != "hissabeth")
            | {name, version, source: (.repository // "https://crates.io/crates/\(.name)")}
        ]
            | unique_by([.name, .version])
    }
    | select(.packages | length > 0)
]
