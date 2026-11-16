//! Admission for fixed public-target relation windows, independent of the compiler.
use super::*;

pub(super) fn function_name(window: &Window, primitive: Primitive, by: &[String]) -> String {
    format!(
        "{}_{}_by_{}{}",
        if primitive.descending() {
            "last"
        } else {
            "first"
        },
        window.name,
        by.join("_"),
        if primitive.many() { "_many" } else { "" }
    )
}

fn scalar(stored: &Stored) -> bool {
    matches!(
        stored,
        Stored::Int { .. } | Stored::String { .. } | Stored::Bool
    ) || matches!(stored,Stored::Cases(cases) if cases.iter().all(|case|case.carries.is_empty()))
}

fn eligible(source: &Table, window: &Window, by: &[String]) -> bool {
    let path = |fields: &[String]| {
        fields.get(by.len()) == Some(&window.source_field) && fields.get(..by.len()) == Some(by)
    };
    !by.is_empty() && (path(&source.key) || source.indexes.iter().any(|index| path(&index.fields)))
}

impl Manifest {
    pub(super) fn related_target<'a>(
        &'a self,
        source: &'a Table,
        function: &Function,
    ) -> Result<(&'a Window, &'a Table), String> {
        let name = function
            .relation
            .as_ref()
            .ok_or("tables.json: a relation read names no relation")?;
        let window = source
            .windows
            .iter()
            .find(|w| w.name == *name)
            .ok_or_else(|| format!("tables.json: {} has no relation {name}", source.name))?;
        let target = self.window_target(source, window)?;
        if !eligible(source, window, &function.by) {
            return Err(format!(
                "tables.json: {} relation {name} has no declared selector/match lookup",
                source.name
            ));
        }
        Ok((window, target))
    }

    fn window_target<'a>(&'a self, source: &Table, window: &Window) -> Result<&'a Table, String> {
        let refused =
            |why: &str| format!("tables.json: {} window {} {why}", source.name, window.name);
        let target = self
            .tables
            .iter()
            .find(|target| target.name == window.target)
            .ok_or_else(|| refused("names no declared target table"))?;
        if target.owner.is_some() || !target.private.is_empty() || !target.shares.is_empty() {
            return Err(refused("requires a public target table"));
        }
        if source.source.as_ref().is_none_or(|s| s.trim().is_empty())
            || source.source != target.source
        {
            return Err(refused("requires the same explicit source on both tables"));
        }
        if target.order.is_none() {
            return Err(refused("requires a declared target order"));
        }
        let from = source
            .fields
            .iter()
            .find(|field| field.name == window.source_field)
            .ok_or_else(|| refused("names no source match field"))?;
        let to = target
            .fields
            .iter()
            .find(|field| field.name == window.target_field)
            .ok_or_else(|| refused("names no target match field"))?;
        if from.stored != to.stored || !scalar(&from.stored) {
            return Err(refused(
                "match fields differ in stored kind/refinement or are not scalar",
            ));
        }
        if target.key.first() != Some(&window.target_field)
            && !target
                .indexes
                .iter()
                .any(|index| index.fields.first() == Some(&window.target_field))
        {
            return Err(refused("has no declared target match lookup"));
        }
        let paths = std::iter::once(source.key.as_slice())
            .chain(source.indexes.iter().map(|index| index.fields.as_slice()));
        let mut found = false;
        for path in paths {
            if let Some(at) = path.iter().position(|field| field == &window.source_field)
                && at > 0
            {
                let by = &path[..at];
                if source.owner.is_none()
                    || by.first() == source.owner.as_ref()
                    || by
                        .first()
                        .is_some_and(|first| source.shares.contains(first))
                {
                    found = true;
                    if source.private.contains(&window.source_field)
                        && by
                            .first()
                            .is_some_and(|first| source.shares.contains(first))
                    {
                        return Err(refused("reveals a private source match field to a sharer"));
                    }
                }
            }
        }
        if !found {
            return Err(refused("has no declared source selector/match lookup"));
        }
        Ok(target)
    }

    pub(super) fn relations_whole(&self) -> Result<(), String> {
        for source in &self.tables {
            let mut names = BTreeSet::new();
            for window in &source.windows {
                if !names.insert(wit_name(&window.name)) {
                    return Err(format!(
                        "tables.json: {} repeats relation {}",
                        source.name, window.name
                    ));
                }
                self.window_target(source, window)?;
            }
        }
        Ok(())
    }
}
