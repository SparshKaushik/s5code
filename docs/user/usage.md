# Usage and costs

The Usage page shows what your coding agents have actually consumed: tokens, cost, and which
models did the work.

Numbers come from the agent CLIs' own session transcripts on disk, plus Cursor's account-wide
usage API. They do not come from S5 Code's history. Work you did outside S5 Code counts too, and
usage from before you installed S5 Code is already there the first time you open the page.

Cost is what the same work would cost at published API rates. It is not what you were billed. If
you are on a subscription, treat it as the value you got out of it rather than a statement.

Use **Past 24h** for an hourly chart covering the exact rolling 24-hour period. The **7 days**,
**30 days**, and **90 days** ranges use daily resolution. Cost and token toggles update both the
headline and chart, and refreshing rescans every connected environment.

## What is counted

| Agent       | Read from                                                     |
| ----------- | ------------------------------------------------------------- |
| Codex       | `~/.codex/sessions`                                           |
| Claude Code | `~/.claude/projects`                                          |
| Cursor      | `cursor.com` using the signed-in Cursor desktop app's session |
| pi          | `~/.pi/agent/sessions`                                        |

Cursor usage is account-wide, so it includes work from every machine signed into that Cursor
account. S5 Code reads Cursor's local auth database read-only, sends the derived first-party
session only to `cursor.com`, and never sends it to a client. If several environments use the same
Cursor account, that account is counted once.

If you connect several environments, the page adds them together. Two environments sharing one
transcript directory, or the same Cursor account, are counted once and the page says so.

Cursor's usage endpoint requires a valid desktop-app session. If Cursor is signed out or that
session has expired, sign in or restart Cursor, then refresh the Usage page.

## Where cost comes from

In order of preference:

1. **The transcript said so.** Some agents record what a response cost. That figure wins.
2. **We know the model.** The model is matched against published rates and priced from its tokens.
3. **You told us.** See [Tagging a model](#tagging-a-model).
4. **Unpriced.** Tokens are counted, cost is not.

The summary at the bottom of the page breaks down which of these your numbers rest on.

## Tagging a model

Agents that route through a gateway report whatever name the gateway uses. Sometimes that is a
name no rate table knows, like `cline-free/glm-5.2`. Sometimes it is a name several vendors sell
at different prices — `claude-opus-5` is served by about a dozen providers, and the cheapest and
the dearest are 20% apart.

Rather than guess and quietly show you a wrong number, those rows are left unpriced with a **Tag**
button. Tag it, and:

- its tokens are priced at that model's rates
- rows you tagged as the same model merge into one line, so a model you reach through three
  gateways reads as one model

Tags are yours, not an environment's: tag a model once and it applies everywhere you connect.

To change or remove a tag, use the **Tagged** button on the row. Removing a tag returns that usage
to unpriced.

Tagging is per gateway on purpose. Two gateways selling `claude-opus-5` may charge differently, so
tagging one leaves the other alone.

## Why a model shows as unpriced

- It is served by several vendors at prices that disagree, so no single answer is right.
- It is a gateway-specific name that no public rate table lists.
- It is new enough that rates have not been published yet.

All three are fixable by tagging.

## Why numbers can differ from your provider's dashboard

- **Rounding and rate changes.** Rates are the published ones as of the last refresh.
- **Subscriptions.** A plan does not bill per token, so the figure here is an equivalent, not a charge.
- **Cache.** Cached input is cheaper than fresh input, and the page shows what caching saved you
  separately. When an adapter such as Kiro reports only rolling context size, S5 Code simulates a
  conservative cache split and labels that model **Estimated**; it is an API-rate estimate, not a
  provider-reported bill.
- **Free tiers and credits.** Not modelled. Usage on a free tier still shows a cost.
