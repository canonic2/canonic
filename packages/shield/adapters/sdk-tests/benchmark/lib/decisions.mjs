// Scenario-owned approval answers. The agent never sees these decisions in advance.

/** Names the offer a host confirmation describes, from the scenario's `offers` patterns; otherwise `other`. */
export function offerClassifier(patterns = {}) {
  const rules = Object.entries(patterns).map(([kind, pattern]) => [kind, new RegExp(pattern, 'i')]);
  return text => rules.find(([, pattern]) => pattern.test(text))?.[0] ?? 'other';
}

/** Answers host requests from one decision set and logs each answer. */
export function decisions(scenario, set) {
  const table = scenario.decisions[set];
  if (!table) throw new Error(`Unknown decision set ${set}; choose one of ${Object.keys(scenario.decisions).join(', ')}`);
  const classify = offerClassifier(scenario.offers);
  const log = [];
  return {
    log,
    /** Ordinary host permission prompts for the user's task are approved once. */
    command(host, detail) {
      log.push({ at: new Date().toISOString(), host, kind: 'command', action: 'accept', detail });
      return 'accept';
    },
    /** Policy confirmations follow the scenario's table for this offer kind. */
    offer(host, text, detail) {
      const kind = classify(text);
      const action = table[kind] ?? table.other ?? 'decline';
      log.push({ at: new Date().toISOString(), host, kind: 'offer', offer: kind, action, text, detail });
      return action;
    },
    unsupported(host, detail) {
      log.push({ at: new Date().toISOString(), host, kind: 'unsupported', action: 'decline', detail });
    },
  };
}
