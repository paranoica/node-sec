# Glossary — node-sec (anti-fraud decision engine)

> Unambiguous domain vocabulary. **If code uses a term differently, that is a bug.**
> _Last updated: 2026-06-17._

## Decisioning core

<!-- @anchor term:decision-engine -->
**Decision engine** — the synchronous service that receives a transaction event and returns a fraud decision within the latency SLA. Owns rule evaluation, feature lookup, model inference, and action selection. The "hot path".

<!-- @anchor term:decision -->
**Decision** — the verdict emitted for one transaction: `action ∈ {approve, decline, step-up, review, hold}` plus a risk score, the risk band, and reason codes. Immutable once emitted; persisted to the audit log.

<!-- @anchor term:action -->
**Action** — the operational outcome of a decision. `approve` (let through), `decline` (hard block), `step-up` (escalate to a challenge), `review` (send to the analyst queue), `hold` (delay/freeze pending review).

<!-- @anchor term:risk-score -->
**Risk score** — a calibrated probability of fraud in [0,1] produced by fusing rule signals and model output. Calibrated so it equals true fraud probability (prerequisite for expected-value decisioning).

<!-- @anchor term:risk-band -->
**Risk band** — a contiguous range of risk score mapped to an action (e.g. low→approve, mid→step-up, high→review, very-high→decline). Bands are configurable per vertical and per merchant.

<!-- @anchor term:reason-code -->
**Reason code** — a stable, human-readable code attached to a decision explaining the dominant contributing signals (rule hits and SHAP-derived model contributions). Required for disputes, regulatory adverse-action, and analyst triage.

<!-- @anchor term:step-up -->
**Step-up** — escalating friction instead of declining: a challenge (3DS / OTP / out-of-band confirmation) whose outcome is `pass | fail | abandon`. Converts a hard decline into a recoverable event.

<!-- @anchor term:expected-value-decisioning -->
**Expected-value decisioning** — choosing the action that minimises expected cost: `EV(action) = P(fraud)·cost_if_fraud + P(legit)·cost_if_friction`, computed from the calibrated risk score and a cost matrix. The principled alternative to a fixed threshold.

## Architecture

<!-- @anchor term:two-path-architecture -->
**Two-path architecture** — the system split into a synchronous decision path (returns a verdict under SLA) and an asynchronous update path (recomputes features/state from the event stream). A CQRS split: the decision request never blocks on state writes.

<!-- @anchor term:hot-path -->
**Hot path** — the latency-critical request→verdict code on the decision engine (Rust + tokio + in-process ONNX). Performs only reads, cheap derivations, rule evaluation, inference, and action selection.

<!-- @anchor term:async-update-path -->
**Async update path** — the off-critical-path stream processor that consumes transaction events, recomputes windowed aggregates and graph/entity state, and materialises them into the online feature store.

<!-- @anchor term:latency-sla -->
**Latency SLA** — the binding non-functional target: p99 < 20 ms at a sustained ~20,000 transactions/second on the hot path. A decision must always return within budget, degrading rather than timing out.

<!-- @anchor term:fail-safe-degradation -->
**Fail-safe degradation** — when a dependency (online feature store, model) exceeds its per-call timeout, the hot path falls back to rules-only (or last-known-good cached features) and still returns a decision within the latency SLA. A degraded decision beats a timeout.

<!-- @anchor term:idempotency-key -->
**Idempotency key** — a client-supplied token making a retried decision request safe: the same key returns the same decision and never double-updates state.

<!-- @anchor term:deterministic-replay -->
**Deterministic replay** — given the same inputs and the same model/rule versions, the engine reproduces the identical decision. Enabled by logging the full feature snapshot and versions; required for audit and debugging.

<!-- @anchor term:audit-log -->
**Audit log** — an immutable, append-only record of every decision: inputs, feature snapshot, rule hits, model version, score, reason codes, and timestamp. A first-class regulatory artifact, retained per policy, not a byproduct of logging.

<!-- @anchor term:entity-key -->
**Entity key** — the identifier an aggregate or graph node is keyed by: card, account, device, IP, merchant, BIN, or counterparty. Event streams are partitioned by entity key for locality.

## Feature store & features

<!-- @anchor term:feature-store -->
**Feature store** — the subsystem owning feature definitions and serving them in two stores: an online store (hot-path reads) and an offline store (training). One definition feeds both.

<!-- @anchor term:online-feature-store -->
**Online feature store** — a low-latency store (Redis) holding precomputed per-entity aggregates the hot path reads in single-digit milliseconds; updated incrementally by the async update path.

<!-- @anchor term:offline-feature-store -->
**Offline feature store** — a columnar/relational store of the same features over history, used for training with point-in-time-correct joins.

<!-- @anchor term:online-offline-parity -->
**Online/offline parity** — the invariant that a feature computed offline for training equals the feature computed online at serving time. Violation is train/serve skew, which silently degrades the model.

<!-- @anchor term:velocity-feature -->
**Velocity feature** — the rate of events over a short sliding window for an entity key (e.g. transactions per card per 5 min, failed-auth count per device per hour). The classic fraud signal.

<!-- @anchor term:aggregate-feature -->
**Aggregate feature** — a count, sum, mean, or distinct-count over an entity key and a window (e.g. distinct merchants per card per 24 h, summed amount per account per 7 d).

<!-- @anchor term:profile-feature -->
**Profile feature** — a static or slow-moving entity attribute (BIN, issuing country, merchant category, account age, device fingerprint, email-domain heuristic).

<!-- @anchor term:deviation-feature -->
**Deviation feature** — a request-time comparison of the current event against the entity's own baseline (amount ÷ rolling mean, geo distance from usual, z-score). Cheap and pure; computed on the hot path.

<!-- @anchor term:sliding-window -->
**Sliding window** — an overlapping time window (1 m, 5 m, 1 h, 24 h, 7 d, 30 d) over which aggregate and velocity features are maintained as per-entity state in the async update path.

<!-- @anchor term:point-in-time-correctness -->
**Point-in-time correctness** — joining a training label only to feature values as they existed at the event timestamp, never using future data. Prevents label leakage that inflates offline metrics.

<!-- @anchor term:feature-freshness -->
**Feature freshness** — how recently an online feature reflects reality (target: sub-second after the event). Staleness directly causes missed velocity-based fraud.

## ML, labels, lifecycle

<!-- @anchor term:risk-model -->
**Risk model** — a trained classifier (baseline LightGBM gradient-boosted trees) scoring a feature vector to a fraud probability; trained in Python, exported to ONNX, run in-process on the hot path.

<!-- @anchor term:onnx -->
**ONNX** — the open model-exchange format used to ship a trained model from Python into the Rust hot path for in-process inference, with no model-server network hop.

<!-- @anchor term:in-process-inference -->
**In-process inference** — running model scoring inside the decision engine process (ONNX Runtime), avoiding a network round-trip to a separate model server.

<!-- @anchor term:score-calibration -->
**Score calibration** — mapping raw model output to a true probability via isotonic regression or Platt scaling, so risk scores are comparable across models and usable in expected-value decisioning.

<!-- @anchor term:class-imbalance -->
**Class imbalance** — the extreme rarity of fraud (~0.1–1% of transactions) that makes accuracy meaningless and requires cost-sensitive training and imbalance-aware metrics.

<!-- @anchor term:cost-sensitive-learning -->
**Cost-sensitive learning** — training that weights false-positive and false-negative errors by their real business cost (class weights, focal loss) rather than optimising raw error count.

<!-- @anchor term:label -->
**Label** — the ground-truth fraud/legit outcome for a transaction. Two streams exist: fast, noisy investigator labels and slow, authoritative chargeback labels.

<!-- @anchor term:chargeback -->
**Chargeback** — a dispute-driven reversal arriving weeks-to-months after a transaction; the authoritative but delayed fraud label. The source of the delayed-label problem.

<!-- @anchor term:delayed-label -->
**Delayed label** — a label that becomes known only long after the decision (chargebacks). Means recent data is only partially labelled and concept drift is hard to detect before labels land.

<!-- @anchor term:reject-inference -->
**Reject inference** — estimating the would-be outcome of transactions that were blocked (and so produced no real-world label), to de-bias training against the engine's own past decisions.

<!-- @anchor term:random-control-holdout -->
**Random control holdout** — a small fraction of traffic allowed through unscored (or scored-but-not-acted) to obtain unbiased labels and break the feedback loop that otherwise poisons training.

<!-- @anchor term:champion-challenger -->
**Champion-challenger** — a live "champion" model serving decisions while one or more "challenger" models run in shadow on the same traffic; a challenger is promoted only when it demonstrably beats the champion.

<!-- @anchor term:shadow-scoring -->
**Shadow scoring** — scoring live traffic with a model whose output is logged but does not affect decisions; used to compare a challenger against the champion safely.

<!-- @anchor term:model-registry -->
**Model registry** — the versioned store of trained models with staging (shadow → challenger → champion); every deployed model is pinned to a feature-definition version and referenced by decisions.

<!-- @anchor term:concept-drift -->
**Concept drift** — the shift over time in fraud tactics and customer behaviour that degrades a fixed model; monitored via feature and score drift as an early proxy before labels arrive.

<!-- @anchor term:psi -->
**PSI (Population Stability Index)** — a drift metric on a feature or score distribution: < 0.1 stable, 0.1–0.25 moderate, > 0.25 significant (retrain trigger).

<!-- @anchor term:pr-auc -->
**PR-AUC** — area under the precision-recall curve (average precision); the primary threshold-free evaluation metric under extreme class imbalance, where ROC-AUC misleads.

<!-- @anchor term:recall-at-fpr -->
**Recall at fixed FPR** — fraud catch rate (true-positive rate) measured at the low false-positive rate that review capacity permits; the operationally meaningful catch metric.

<!-- @anchor term:precision-at-n -->
**Precision at N** — precision among the top-N riskiest items when only N can be investigated; models the review-capacity constraint directly.

<!-- @anchor term:alert-to-fraud-ratio -->
**Alert-to-fraud ratio** — alerts raised per true fraud caught; the operational health metric governing analyst workload.

## Rules engine

<!-- @anchor term:rules-engine -->
**Rules engine** — the deterministic component evaluating analyst-authored rules against an event and its features. Rules are data (hot-reloadable config), not code, so thresholds change without redeploy.

<!-- @anchor term:rule -->
**Rule** — a parameterised deterministic predicate over an event plus its features, producing a hard override (force decline/approve) or a soft signal that feeds the score. Carries a severity and a reason code.

<!-- @anchor term:blocklist -->
**Blocklist** — a deny list of known-bad identifiers (PAN, BIN, device fingerprint, IP, email, address, counterparty, sanctioned address). A hit is a hard override.

<!-- @anchor term:velocity-rule -->
**Velocity rule** — a rule firing when a velocity or aggregate feature crosses a threshold over a window (e.g. > 5 declines per card per 10 min).

<!-- @anchor term:impossible-travel -->
**Impossible travel** — a rule firing when two transactions for one entity occur in geographies too far apart to be reachable in the elapsed time.

<!-- @anchor term:amount-anomaly -->
**Amount anomaly** — a signal that the transaction amount deviates strongly from the entity's baseline (z-score) or matches a suspicious pattern (round numbers, just-below a threshold).

<!-- @anchor term:mcc -->
**MCC (Merchant Category Code)** — the code classifying a merchant's business type; certain categories (gambling, money transfer, crypto, precious metals) carry elevated risk.

## Card-vertical typologies

<!-- @anchor term:card-testing -->
**Card testing** — validating stolen card numbers with bursts of small or zero-value authorisations, often distributed across many cards from one device/IP, with a high decline-retry rate.

<!-- @anchor term:bin-attack -->
**BIN attack** — enumerating valid card numbers within an issuer BIN range by brute-forcing expiry/CVV, visible as many distinct PANs sharing a BIN tested in a short window.

<!-- @anchor term:decline-retry-storm -->
**Decline-retry storm** — a rapid sequence of declined authorisations retried with small variations, a hallmark of card testing and enumeration.

<!-- @anchor term:avs-cvv-mismatch -->
**AVS/CVV mismatch** — a failure of the address-verification (AVS) or card-verification-value (CVV) check; a soft risk signal for CNP fraud.

<!-- @anchor term:cnp-fraud -->
**CNP fraud** — card-not-present fraud: use of stolen card data where no physical card/EMV check occurs, caught via credential, device, velocity, and behavioural signals.

<!-- @anchor term:friendly-fraud -->
**Friendly fraud** — a legitimate cardholder disputing a genuine purchase (first-party fraud / chargeback abuse); authenticated-but-bad, needing history and dispute features rather than credential signals.

## Entities

<!-- @anchor term:transaction -->
**Transaction** — the unit of evaluation: a money-movement event with amount (integer minor units), currency, timestamp, the entities involved (card/account/device/IP/merchant/counterparty), and channel. The input to a decision.

<!-- @anchor term:account -->
**Account** — a customer-held account that sends or receives funds; carries age, KYC risk rating, and behavioural baseline.

<!-- @anchor term:card -->
**Card** — a payment instrument identified by PAN, with a BIN, issuing bank, and country.

<!-- @anchor term:bin -->
**BIN** — the bank identification number (leading PAN digits) identifying the issuer; the keyed entity for BIN-attack detection.

<!-- @anchor term:device -->
**Device** — a client device identified by a fingerprint; shared devices across accounts are a linkage signal.

<!-- @anchor term:ip-address -->
**IP address** — the network origin of a request; used for geo, velocity, and linkage signals.

<!-- @anchor term:merchant -->
**Merchant** — the payee of a card transaction, classified by MCC; carries a risk profile.

<!-- @anchor term:counterparty -->
**Counterparty** — the recipient of a P2P or crypto transfer (a payee account or an on-chain address).

## Graph & entity resolution

<!-- @anchor term:entity-resolution -->
**Entity resolution** — linking and deduplicating records that refer to the same real-world entity (via shared name, ID, address, phone, email, device, card), producing the identity graph. Pipeline: normalise → block → pairwise match → cluster.

<!-- @anchor term:identity-graph -->
**Identity graph** — the graph linking accounts/cards/devices/phones/emails/addresses that belong to, or are shared by, the same entities; the substrate for ring and mule detection.

<!-- @anchor term:transaction-graph -->
**Transaction graph** — the directed, time-stamped graph of money flows between entities (nodes = entities, edges = transfers weighted by amount/count/recency).

<!-- @anchor term:fraud-ring -->
**Fraud ring** — a coordinated group of colluding entities, appearing as a moderate disjoint community, a directed cycle, or a bipartite mule structure in the graph.

<!-- @anchor term:mule-account -->
**Mule account** — an account that receives and quickly forwards illicit funds; signature = fan-in then fan-out, pass-through ratio near 1, short dwell time, age-vs-activity mismatch, dormant-then-active.

<!-- @anchor term:fan-in-fan-out -->
**Fan-in/fan-out** — a money-flow structure that concentrates from many sources (fan-in) then disperses to few destinations (fan-out), or vice versa; a core laundering and mule motif.

<!-- @anchor term:pass-through-ratio -->
**Pass-through ratio** — the share of inbound funds debited out again within a short window (Σout ÷ Σin); a value near 1 marks a conduit account.

<!-- @anchor term:community-detection -->
**Community detection** — a graph algorithm (e.g. Louvain) clustering densely connected nodes; fraud rings surface as moderate communities separate from the legitimate giant component.

<!-- @anchor term:pagerank -->
**PageRank** — a centrality measure of node influence; the personalised variant, seeded from known-bad nodes, propagates risk along the graph.

<!-- @anchor term:shortest-path-to-bad -->
**Shortest-path-to-bad** — the graph distance from a node to the nearest known-bad (sanctioned / confirmed-fraud / SAR'd) node; short paths propagate risk.

<!-- @anchor term:temporal-motif -->
**Temporal motif** — a time-ordered subgraph pattern (causally ordered edges) such as scatter-gather or a directed cycle; beats static structure for detecting layering.

## Compliance

<!-- @anchor term:compliance-layer -->
**Compliance layer** — the subsystem enforcing regulatory controls: sanctions screening, AML transaction monitoring, the case lifecycle, SAR/STR generation, four-eyes, and the audit trail.

<!-- @anchor term:sanctions-screening -->
**Sanctions screening** — matching entities against watchlists (OFAC SDN, PEP, adverse media) using fuzzy and phonetic name matching, with false-positive reduction via secondary identifiers; run real-time and in batch on list-delta.

<!-- @anchor term:ofac-sdn -->
**OFAC SDN** — the US Specially Designated Nationals sanctions list; a confirmed match forces a block/freeze and a report. Lists are mutable, so screening is date-versioned.

<!-- @anchor term:pep -->
**PEP** — Politically Exposed Person; an elevated-risk customer category (including relatives and close associates) driving enhanced due diligence.

<!-- @anchor term:name-matching -->
**Name matching** — fuzzy (Jaro-Winkler, Levenshtein) and phonetic (Soundex, Metaphone) comparison of names against watchlists, tuned to cut false positives without missing true matches.

<!-- @anchor term:aml-monitoring -->
**AML monitoring** — transaction monitoring against parameterised typology rules over windowed aggregates and graph motifs, each alert tagged with its hypothesised laundering typology.

<!-- @anchor term:structuring -->
**Structuring** — splitting a sum into many sub-threshold transactions to evade reporting (e.g. multiple deposits just below the CTR threshold); detected by just-below-threshold density over a window. "Smurfing" is structuring executed across multiple actors/accounts.

<!-- @anchor term:funnel-account -->
**Funnel account** — one account receiving deposits in many geographies and withdrawn/transferred elsewhere; geographic dispersion of credits vs concentration of debits.

<!-- @anchor term:round-tripping -->
**Round-tripping** — sending funds out (often offshore) then receiving them back disguised as legitimate inflow of matching size within a window.

<!-- @anchor term:cuckoo-smurfing -->
**Cuckoo smurfing** — hijacking a legitimate expected inbound transfer by routing dirty cash through unrelated deposits matching its amount/timing, often without the beneficiary's knowledge.

<!-- @anchor term:dormant-then-active -->
**Dormant-then-active** — a long-inactive account that suddenly transacts heavily; a mule and laundering red flag.

<!-- @anchor term:typology -->
**Typology** — a named fraud or laundering pattern (e.g. structuring, pig-butchering, card testing) with a characteristic data footprint; every alert carries its hypothesised typology.

<!-- @anchor term:sar -->
**SAR/STR** — a Suspicious Activity / Transaction Report: the regulatory filing of suspicion, produced through a maker-checker workflow with filing deadlines and continuing-activity follow-ups; subjects must never be tipped off.

<!-- @anchor term:ctr -->
**CTR** — Currency Transaction Report: a mandatory, non-discretionary report of cash movement above a fixed threshold (US $10,000, single or same-day aggregated). Distinct from a suspicion-based SAR.

<!-- @anchor term:case-lifecycle -->
**Case lifecycle** — the alert state machine: alert → triage (dedupe, enrich, prioritise, auto-close low-risk) → investigate → close / escalate / file-SAR → dispositioned (immutable, audit-logged).

<!-- @anchor term:four-eyes -->
**Four-eyes** — the maker-checker control requiring that the analyst who drafts a SAR is not the one who approves/files it (maker ≠ checker).

<!-- @anchor term:review-queue -->
**Review queue** — the risk-prioritised queue of alerts/cases routed to analysts for investigation; its capacity bounds how many decisions can be `review`.

## Verticals

<!-- @anchor term:card-vertical -->
**Card vertical** — the payments/card fraud domain (CNP, card testing, BIN attacks, friendly fraud); the lead vertical for the end-to-end slice (phases 1–3).

<!-- @anchor term:p2p-vertical -->
**P2P vertical** — the person-to-person push-payment domain (APP fraud, mule networks, Confirmation of Payee, inbound monitoring).

<!-- @anchor term:crypto-vertical -->
**Crypto vertical** — the cryptocurrency domain (on-chain laundering, mixers, scam tokens, sanctioned-address exposure, exchange-side deposit/withdrawal controls).

<!-- @anchor term:vertical-pack -->
**Vertical pack** — a bundle of vertical-specific feature definitions, rules, typologies, and decision actions that plug into the shared engine core for one vertical.

<!-- @anchor term:app-fraud -->
**APP fraud** — authorized push payment fraud: the payer is socially engineered to authorise the payment themselves, so it passes authentication; sub-types include purchase, investment, romance, and impersonation scams.

<!-- @anchor term:confirmation-of-payee -->
**Confirmation of Payee** — a pre-payment account-name check returning match / close-match / no-match / unavailable, surfaced to the payer before sending to interrupt APP fraud.

<!-- @anchor term:coercion-signal -->
**Coercion signal** — a behavioural-biometric indicator of a socially-engineered payment (segmented typing, session dead-time, hesitation, payment right after an inbound call) suggesting the payer is under duress.

<!-- @anchor term:on-chain-ledger -->
**On-chain ledger** — the simulated blockchain ledger (account/UTXO model) over which crypto-vertical clustering, tracing, and exposure scoring operate.

<!-- @anchor term:address-clustering -->
**Address clustering** — grouping on-chain addresses likely controlled by one entity (common-input-ownership, change-address heuristics), filtering CoinJoin to avoid false merges.

<!-- @anchor term:taint-tracing -->
**Taint tracing** — propagating "dirtiness" from illicit funds through the transaction graph; FIFO (first-in-first-out) tracing gives lossless, bidirectional ancestry.

<!-- @anchor term:exposure-scoring -->
**Exposure scoring** — scoring an address/transaction by its direct and indirect (multi-hop) exposure to known-bad clusters (mixers, sanctioned, darknet, scam), without a fixed hop cutoff.

<!-- @anchor term:mixer -->
**Mixer** — a service (smart-contract or custodial) pooling many users' coins to sever the deposit↔withdrawal link; high inflow share from illicit sources.

<!-- @anchor term:peel-chain -->
**Peel chain** — a chain of transactions repeatedly forwarding the bulk onward while peeling off small amounts, fragmenting proceeds across many addresses to mimic change behaviour.

<!-- @anchor term:chain-hopping -->
**Chain hopping** — moving funds across different blockchains via bridges/swaps to break the trail, forcing identity reconstruction across heterogeneous ledgers.

<!-- @anchor term:scam-token -->
**Scam token** — a fraudulent token (rug pull or honeypot): a rug pull absconds with pooled value; a honeypot token is engineered so victims can buy but not sell.

<!-- @anchor term:approval-phishing -->
**Approval phishing** — tricking a wallet owner into signing a token `approve`/`permit` so the attacker drains funds via `transferFrom`; the gasless `permit` variant leaves no on-chain approval record.

<!-- @anchor term:address-poisoning -->
**Address poisoning** — seeding a victim's history with a lookalike address (matching prefix/suffix) so they copy-paste the attacker's address on the next transfer.

<!-- @anchor term:travel-rule -->
**Travel Rule** — the FATF Recommendation 16 requirement that originator and beneficiary data accompany virtual-asset transfers at or above a de-minimis threshold between service providers.

<!-- @anchor term:sanctioned-address -->
**Sanctioned address** — an on-chain address on a sanctions list (OFAC crypto SDN); screened date-versioned, since addresses are listed and delisted over time.

## Simulation & infrastructure

<!-- @anchor term:simulation-harness -->
**Simulation harness** — the synthetic environment driving the system: a transaction generator, fraud-pattern injection per typology, ground-truth and delayed-label emission, and a control dashboard.

<!-- @anchor term:synthetic-transaction-generator -->
**Synthetic transaction generator** — the component emitting a configurable stream of legitimate baseline traffic and injected fraud, with controllable rate, mix, and entity population, into the event backbone.

<!-- @anchor term:fraud-pattern-injection -->
**Fraud pattern injection** — seeding the synthetic stream with parameterised instances of named typologies (card testing, structuring, mule rings, pig-butchering) carrying ground-truth labels for evaluation.

<!-- @anchor term:ground-truth-label -->
**Ground-truth label** — the simulator-known fraud/legit truth for a generated transaction, used to evaluate detection; in production this is approximated by chargeback and investigator labels.

<!-- @anchor term:event-backbone -->
**Event backbone** — the durable, partitioned event log (Redpanda, Kafka-compatible) carrying transaction events to the async update path; partitioned by entity key.

<!-- @anchor term:observability -->
**Observability** — the metrics/tracing layer (Prometheus + Grafana): hot-path latency percentiles (p50/p99/p999), throughput, decision mix, score and feature drift.
