//! Stateless paging of bounded inspection collections.
use super::*;
use crate::review::page;

impl Views {
    pub(in crate::review::mcp) fn page(
        &mut self,
        context: &Context,
        name: &str,
        args: &Value,
        data: Value,
        field: &str,
    ) -> Result<Value, Error> {
        let query = page::query(context, name, args);
        if name == "pack_info" && field == "rules" {
            page::bounded_with(name, &query, data, field, crate::review::packs::rule_view)
        } else {
            page::bounded(name, &query, data, field)
        }
    }
}
