use std::{collections::BTreeSet, io};
use toml_edit::{Document, Item, TableLike};

pub(super) fn merge(text: &str, before: &toml::Table, after: &toml::Table) -> io::Result<String> {
    let mut document = text
        .parse::<Document>()
        .map_err(|_| io::Error::other("plugin file parse failed"))?;
    let generated = toml::to_string_pretty(after)
        .map_err(|_| io::Error::other("plugin file serialization failed"))?;
    let source = generated
        .parse::<Document>()
        .map_err(|_| io::Error::other("plugin file serialization failed"))?;
    reconcile(
        document.as_table_mut(),
        source.as_table(),
        before,
        after,
        false,
    )?;
    Ok(document.to_string())
}
fn reconcile(
    destination: &mut dyn TableLike,
    source: &dyn TableLike,
    before: &toml::Table,
    after: &toml::Table,
    inline: bool,
) -> io::Result<()> {
    for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        let a = before.get(key);
        let b = after.get(key);
        if a == b {
            continue;
        }
        let Some(new) = source.get(key) else {
            destination.remove(key);
            continue;
        };
        if let (Some(a), Some(b), Some(existing)) = (
            a.and_then(toml::Value::as_table),
            b.and_then(toml::Value::as_table),
            destination.get_mut(key),
        ) {
            let inline = existing.is_inline_table();
            if let (Some(destination), Some(source)) =
                (existing.as_table_like_mut(), new.as_table_like())
            {
                reconcile(destination, source, a, b, inline)?;
                continue;
            }
        }
        let mut replacement = new.clone();
        if inline {
            replacement = Item::Value(
                replacement
                    .into_value()
                    .map_err(|_| io::Error::other("plugin file table conversion failed"))?,
            );
        }
        if let (Some(old), Some(new)) = (
            destination.get(key).and_then(Item::as_value),
            replacement.as_value_mut(),
        ) {
            *new.decor_mut() = old.decor().clone();
        }
        destination.insert(key, replacement);
    }
    Ok(())
}
