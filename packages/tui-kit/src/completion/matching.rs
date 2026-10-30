//! Deterministic prefix, word-start and subsequence matching on Unicode names.
use super::Menu;

pub(super) fn matched(name: &str, query: &str) -> Option<(u8, Vec<usize>)> {
    let folded: Vec<(usize, char)> = name
        .chars()
        .enumerate()
        .flat_map(|(i, c)| c.to_lowercase().map(move |c| (i, c)))
        .collect();
    let name: Vec<char> = folded.iter().map(|(_, c)| *c).collect();
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let prefix_start = usize::from(
        matches!(name.first(), Some('/' | '@')) && !matches!(query.first(), Some('/' | '@')),
    );
    if let Some(start) = name
        .windows(query.len())
        .enumerate()
        .find_map(|(i, w)| (w == query && (i == 0 || !name[i - 1].is_alphanumeric())).then_some(i))
    {
        return Some((
            u8::from(start != prefix_start),
            (start..start + query.len()).map(|i| folded[i].0).collect(),
        ));
    }
    let mut positions = Vec::new();
    let mut cursor = 0;
    for c in query {
        let next = name[cursor..].iter().position(|n| *n == c)? + cursor;
        positions.push(folded[next].0);
        cursor = next + 1;
    }
    Some((2, positions))
}

impl Menu {
    pub(super) fn rank(&mut self, streaming: bool) {
        let old = &self.order;
        // Incoming rows may replace/reorder a source snapshot. Preserve IDs, not indices.
        let mut ranked: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !row.hidden)
            .filter_map(|(i, row)| {
                let name = matched(&row.name, &self.request.query).map(|(rank, _)| rank);
                let rank = if name.is_some_and(|rank| rank < 2) {
                    name
                } else if row
                    .aliases
                    .iter()
                    .any(|alias| matched(alias, &self.request.query).is_some())
                {
                    Some(2)
                } else {
                    name.map(|_| 3)
                };
                rank.map(|rank| (rank, std::cmp::Reverse(row.recency), i))
            })
            .collect();
        ranked.sort_by_key(|entry| *entry);
        self.visible = ranked.into_iter().map(|(_, _, i)| i).collect();
        if streaming && !old.is_empty() {
            self.visible.sort_by_key(|i| {
                old.iter()
                    .position(|id| id == &self.rows[*i].id)
                    .unwrap_or(old.len() + *i)
            });
        }
        self.order = self
            .visible
            .iter()
            .map(|i| self.rows[*i].id.clone())
            .collect();
        if !self.visible.iter().any(|i| {
            Some(&self.rows[*i].id) == self.selected.as_ref() && self.rows[*i].disabled.is_none()
        }) {
            self.selected = self
                .visible
                .iter()
                .find(|i| self.rows[**i].disabled.is_none())
                .map(|i| self.rows[*i].id.clone());
        }
    }
}
