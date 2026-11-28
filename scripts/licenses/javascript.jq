# Reduces license-checker-rseidelsohn's JSON to the same groups as rust.jq.
[
    .[]
    | {
        license: (.licenses | if type == "array" then join(" OR ") else . end),
        text: (.licenseText // ""),
        package: {
            name,
            version,
            source: (
                if (.repository // "") == "" then "https://www.npmjs.com/package/\(.name)"
                else .repository end
            ),
        },
    }
]
| group_by([.license, .text])
| map({license: .[0].license, text: .[0].text, packages: (map(.package) | sort_by(.name, .version))})
| sort_by(.license, .packages[0].name)
