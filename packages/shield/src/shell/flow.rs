//! Conservative control-flow traversal, scope joins and bounded loop evaluation.
use super::structure::Node;
use super::{
    Analysis, Decision, Dialect, Error, State, bindings, control, positional_words, structure,
    taint,
};
use serde_json::json;

impl Analysis<'_> {
    pub(super) fn visit(
        &mut self,
        node: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        (self.checkpoint)()?;
        if depth > 32 {
            return Err(Error::Limit);
        }
        if state.stopped {
            let previous = self.suppressed;
            self.suppressed = true;
            let mut isolated = state.clone();
            isolated.stopped = false;
            let result = self.visit(node, &mut isolated, depth + 1);
            self.suppressed = previous;
            return result;
        }
        match node {
            Node::Opaque { span } => {
                self.complete = false;
                state.brace_expansion = None;
                state.cwd_unknown = true;
                state.environment_unknown = true;
                state.variables.clear();
                state.defined_functions.clear();
                self.record(json!({"span":span,"coverage":"opaque","decision":"ask"}))?;
                Ok(Decision::Ask)
            }
            Node::Command {
                span,
                words,
                redirections,
            } => self.simple_command(*span, words, redirections, state, depth),
            Node::Sequence { children } => {
                let mut decision = Decision::Allow;
                for child in children {
                    decision = decision.max(self.visit(child, state, depth + 1)?);
                }
                Ok(decision)
            }
            Node::Group { subshell, body } => {
                if *subshell {
                    self.isolated(body, &mut state.clone(), depth + 1)
                } else {
                    self.visit(body, state, depth + 1)
                }
            }
            Node::Background { body } => self.background(body, state, depth),
            Node::Decorated { operator, body } if operator.starts_with("coproc") => {
                // The command still runs; its pipes, NAME array and PID
                // variable are not modeled.
                self.complete = false;
                let decision = self.visit(body, state, depth + 1)?;
                state.variables.clear();
                state.environment_unknown = true;
                self.record(json!({"coverage":"coprocess","operator":operator,"decision":"ask"}))?;
                Ok(decision.max(Decision::Ask))
            }
            Node::Decorated { body, .. } => self.visit(body, state, depth + 1),
            Node::Redirected { body, redirections } => {
                let redirects = self.redirections(redirections, &[], state, depth)?;
                Ok(redirects.max(self.visit(body, state, depth + 1)?))
            }
            Node::Binary {
                operator,
                left,
                right,
            } => self.binary(operator, left, right, state, depth),
            Node::Conditional { condition, yes, no } => {
                let a = self.visit(condition, state, depth + 1)?;
                let mut yes_state = state.clone();
                let mut no_state = state.clone();
                let b = self.visit(yes, &mut yes_state, depth + 1)?;
                let c = if let Some(no) = no {
                    self.visit(no, &mut no_state, depth + 1)?
                } else {
                    Decision::Allow
                };
                yes_state.join(&no_state);
                *state = yes_state;
                Ok(a.max(b).max(c))
            }
            Node::Loop {
                condition, body, ..
            } => self.conditional_loop(condition, body, state, depth),
            Node::Function { name, body } => self.define_function(name, body, state),
            Node::For { .. } => self.for_values(node, state, depth),
            Node::ArithmeticLoop {
                header: values,
                body,
            } => self.arithmetic_loop(values, body, state, depth),
            Node::Case { subject, arms } => self.case(subject, arms, state, depth),
            Node::Expression { syntax, words } => self.expression(syntax, words, state, depth),
        }
    }

    /// A pipeline runs both sides isolated; a list joins the right side's state.
    fn binary(
        &mut self,
        operator: &str,
        left: &Node,
        right: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        if matches!(operator, "|" | "|&") {
            // Both branches receive the same input. Bash lastpipe may
            // propagate the last command's state, so taint the successor.
            let mut lhs = state.clone();
            let mut rhs = state.clone();
            // Only a simple command directly after the pipe is known
            // to read it; groups and subshells are not modeled.
            rhs.stdin_pipe = matches!(right, Node::Command { .. });
            let a = self.isolated(left, &mut lhs, depth + 1)?;
            let b = self.isolated(right, &mut rhs, depth + 1)?;
            if rhs.stopped && self.dialect == Dialect::Bash {
                state.variables.clear();
                state.environment_unknown = true;
            }
            state.join(&rhs);
            Ok(a.max(b))
        } else {
            let a = self.visit(left, state, depth + 1)?;
            let mut branch = state.clone();
            let b = self.visit(right, &mut branch, depth + 1)?;
            state.join(&branch);
            Ok(a.max(b))
        }
    }

    fn background(
        &mut self,
        body: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let launch = self.operations.len();
        let decision = self.isolated(body, &mut state.clone(), depth + 1)?;
        state.variables.remove("!");
        if let Node::Command { span, .. } = body {
            let job = super::provenance::Job {
                source_unit: self.source_unit,
                span: *span,
                launch,
            };
            state
                .variables
                .insert("!".into(), super::provenance::Binding::BackgroundJob(job));
            self.record(
                json!({"span":span, "coverage":"background-job", "origin":job, "decision":"allow"}),
            )?;
        }
        Ok(decision)
    }

    pub(super) fn conditional_loop(
        &mut self,
        condition: &Node,
        body: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let mut branch = state.clone();
        self.frames.push(control::Frame::new(control::Kind::Loop));
        taint(condition, &mut branch);
        taint(body, &mut branch);
        let a = self.visit(condition, &mut branch, depth + 1)?;
        let b = self.visit(body, &mut branch, depth + 1)?;
        let frame = self
            .frames
            .pop()
            .ok_or(Error::Contract("loop frame missing"))?;
        frame.merge(&mut branch);
        state.join(&branch);
        Ok(a.max(b))
    }

    pub(super) fn isolated(
        &mut self,
        node: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        state.variables.remove("!");
        self.frames
            .push(control::Frame::new(control::Kind::Process));
        let result = self.visit(node, state, depth);
        self.frames.pop();
        result
    }

    pub(super) fn arithmetic_loop(
        &mut self,
        values: &[structure::Word],
        body: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.complete = false;
        let expansions = self.expansions(values, state, depth)?;
        let mut branch = state.clone();
        self.frames.push(control::Frame::new(control::Kind::Loop));
        branch.environment_unknown = true;
        branch.variables.clear();
        taint(body, &mut branch);
        let decision = self.visit(body, &mut branch, depth + 1)?;
        let frame = self
            .frames
            .pop()
            .ok_or(Error::Contract("loop frame missing"))?;
        frame.merge(&mut branch);
        state.join(&branch);
        Ok(decision.max(expansions).max(Decision::Ask))
    }

    pub(super) fn for_values(
        &mut self,
        node: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let Node::For {
            variable,
            values,
            implicit_positionals,
            keyword,
            body,
        } = node
        else {
            return Err(Error::Contract("loop binding node required"));
        };
        self.complete = false;
        let expansions = self.expansions(values, state, depth)?;
        let resolved = positional_words(values, state, self.dialect, self.checkpoint)?;
        let mut candidates: Vec<_> = if *implicit_positionals {
            state.positionals.clone().unwrap_or_else(|| vec![None])
        } else {
            resolved.iter().map(|word| word.literal.clone()).collect()
        };
        if keyword == "select" && !candidates.is_empty() {
            // Invalid selections leave the selected variable empty; EOF may
            // skip the body. Never simulate user interaction or infinite loops.
            candidates.push(Some(String::new()));
        }
        if keyword == "for" && candidates.iter().all(Option::is_some) {
            return self
                .finite_loop(variable, candidates, body, state, depth)
                .map(|decision| decision.max(expansions));
        }
        let input = state.clone();
        let mut decision = Decision::Ask;
        for value in candidates {
            let mut branch = input.clone();
            self.frames.push(control::Frame::new(control::Kind::Loop));
            branch.environment_unknown = true;
            taint(body, &mut branch);
            if let Some(value) = value.filter(|_| !bindings::dynamic_variable(variable)) {
                branch.variables.insert(variable.into(), value.into());
            } else {
                branch.variables.remove(variable);
            }
            decision = decision.max(self.visit(body, &mut branch, depth + 1)?);
            let frame = self
                .frames
                .pop()
                .ok_or(Error::Contract("loop frame missing"))?;
            frame.merge(&mut branch);
            state.join(&branch);
        }
        Ok(decision.max(expansions))
    }

    pub(super) fn finite_loop(
        &mut self,
        variable: &str,
        values: Vec<Option<String>>,
        body: &Node,
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        let mut decision = Decision::Allow;
        if variable == "PATH" && !values.is_empty() {
            // This binding changes which executable each literal name selects.
            // Keep inspecting the body, but do not treat the binding as inert.
            decision = Decision::Ask;
            self.record(
                json!({"coverage":"loop-execution-setting","variable":variable,"decision":"ask"}),
            )?;
        } else if !values.is_empty()
            && bindings::inherited_name(variable, self.clients.inherited.as_ref())
        {
            // Like a plain assignment, a binding of an exported name reaches
            // every command launched in or after the loop, such as HOME
            // selecting a client's configuration file.
            decision = Decision::Ask;
            self.record(
                json!({"coverage":"shell-assignment","variable":variable,"decision":"ask"}),
            )?;
        }
        let mut breaks = Vec::new();
        for value in values {
            self.frames.push(control::Frame::new(control::Kind::Loop));
            let result = if state.stopped {
                let mut isolated = state.clone();
                bindings::loop_value(variable, value, &mut isolated);
                self.visit(body, &mut isolated, depth + 1)
            } else {
                state.environment_unknown = true;
                bindings::loop_value(variable, value, state);
                self.visit(body, state, depth + 1)
            };
            let frame = self
                .frames
                .pop()
                .ok_or(Error::Contract("loop frame missing"))?;
            decision = decision.max(result?);
            breaks.extend(frame.iteration(state));
        }
        for exit in breaks {
            state.join(&exit);
        }
        Ok(decision)
    }

    pub(super) fn expression(
        &mut self,
        syntax: &str,
        words: &[structure::Word],
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.complete = false;
        state.environment_unknown = true;
        state.variables.clear();
        let decision = self.expansions(words, state, depth)?.max(Decision::Ask);
        self.record(json!({"coverage":"opaque-expression","syntax":syntax,"words":words,"decision":decision}))?;
        Ok(decision)
    }

    pub(super) fn case(
        &mut self,
        subject: &structure::Word,
        arms: &[structure::CaseArm],
        state: &mut State,
        depth: usize,
    ) -> Result<Decision, Error> {
        self.complete = false;
        let mut decision = self
            .expansions(std::slice::from_ref(subject), state, depth)?
            .max(Decision::Ask);
        let input = state.clone();
        let mut fallthrough = None;
        for arm in arms {
            let mut branch = input.clone();
            if let Some(previous) = &fallthrough {
                branch.join(previous);
            }
            decision = decision.max(self.expansions(&arm.patterns, &mut branch, depth)?);
            decision = decision.max(self.visit(&arm.body, &mut branch, depth + 1)?);
            state.join(&branch);
            fallthrough = matches!(arm.terminator.as_str(), ";&" | ";;&").then_some(branch);
        }
        Ok(decision)
    }
}
