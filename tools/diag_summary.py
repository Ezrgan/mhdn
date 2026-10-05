#!/usr/bin/env python3
"""Summarize mhdn diag logs.

Usage: python3 tools/diag_summary.py LOG [LOG...]
"""

import collections
import sys


def read_rows(path):
    """One snapshot read. Incomplete trailing line is ignored."""
    with open(path, "rb") as handle:
        blob = handle.read()
    rows = []
    for raw in blob.splitlines():
        try:
            text = raw.decode("utf-8", "replace")
        except Exception:
            continue
        if not text.startswith("[") or "]" not in text:
            continue
        stamp, _, rest = text.partition("]")
        stamp = stamp[1:].strip()
        try:
            t = float(stamp)
        except ValueError:
            continue
        rows.append((t, rest.strip()))
    return rows


def kv_parse(rest):
    out = {}
    parts = rest.split()
    for tok in parts[1:]:
        if "=" in tok:
            key, val = tok.split("=", 1)
            out[key] = val
    return out


def as_int(value):
    if value is None or value == "-" or value == "":
        return None
    try:
        if value.lower().startswith("0x"):
            return int(value, 16)
        return int(value)
    except ValueError:
        try:
            return int(float(value))
        except ValueError:
            return None


def fmt_t(t):
    return f"{t:.3f}"


def fmt_n(n):
    if n is None:
        return "-"
    if isinstance(n, float) and not n.is_integer():
        return f"{n:.1f}"
    return str(int(n)) if isinstance(n, float) else str(n)


def mean(xs):
    if not xs:
        return None
    return sum(xs) / len(xs)


def percentile(xs, p):
    if not xs:
        return None
    ordered = sorted(xs)
    if len(ordered) == 1:
        return float(ordered[0])
    rank = (len(ordered) - 1) * p
    lo = int(rank)
    hi = min(lo + 1, len(ordered) - 1)
    frac = rank - lo
    return ordered[lo] * (1.0 - frac) + ordered[hi] * frac


def mode_of(xs):
    if not xs:
        return None
    counts = collections.Counter(xs)
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))[0][0]


def pct(part, whole):
    if not whole:
        return "n/a"
    return f"{100.0 * part / whole:.1f}%"


def monster_identity(rest):
    """(slot, addr, species, gen) in slot order. HP is ignored."""
    ident = []
    tokens = rest.split()
    idx = 0
    # Walk slot= groups. Tokens: slot=N addr=A species=S hp=H max_hp=M gen=G
    while idx < len(tokens):
        tok = tokens[idx]
        if tok.startswith("slot="):
            fields = {}
            for step in range(0, 6):
                if idx + step >= len(tokens) or "=" not in tokens[idx + step]:
                    break
                key, val = tokens[idx + step].split("=", 1)
                fields[key] = val
            if {"slot", "addr", "species", "gen"} <= fields.keys():
                ident.append(
                    (fields["slot"], fields["addr"], fields["species"], fields["gen"])
                )
            idx += 6
        else:
            idx += 1
    return tuple(ident)


def species_brief(ident):
    if not ident:
        return "(empty)"
    counts = collections.Counter(species for _, _, species, _ in ident)
    parts = []
    for species, count in counts.items():
        parts.append(species if count == 1 else f"{species}x{count}")
    return ",".join(parts)


def scene_name(rest):
    parts = rest.split()
    return parts[1] if len(parts) > 1 else "?"


def fullscreen_of(rest):
    if rest == "azahar lost" or rest.startswith("azahar lost"):
        return None
    if "fullscreen=true" in rest:
        return True
    if "fullscreen=false" in rest:
        return False
    return "unchanged"


def state_at(events, t, default=None):
    state = default
    for when, value in events:
        if when <= t:
            state = value
        else:
            break
    return state


def split_duration(start, end, events):
    state = state_at(events, start, None)
    cursor = start
    windowed = fullscreen = unknown = 0.0
    for when, value in events:
        if when <= start:
            continue
        if when >= end:
            break
        delta = when - cursor
        if state is True:
            fullscreen += delta
        elif state is False:
            windowed += delta
        else:
            unknown += delta
        cursor = when
        state = value
    delta = end - cursor
    if state is True:
        fullscreen += delta
    elif state is False:
        windowed += delta
    else:
        unknown += delta
    return windowed, fullscreen, unknown


def missing_sec(times, start, end):
    covered = [t for t in times if start <= t <= end]
    holes = 0

    def consume(a, b):
        nonlocal holes
        gap = b - a
        if gap > 1.5:
            skipped = int(round(gap)) - 1
            if skipped > 0:
                holes += skipped

    if not covered:
        consume(start, end)
        return holes
    if covered[0] - start > 1.5:
        consume(start, covered[0])
    for left, right in zip(covered, covered[1:]):
        consume(left, right)
    if end - covered[-1] > 1.5:
        consume(covered[-1], end)
    return holes


def hist_bucket(amount):
    if 1 <= amount <= 49:
        return "1-49"
    if 50 <= amount <= 199:
        return "50-199"
    if 200 <= amount <= 499:
        return "200-499"
    if amount >= 500:
        return "500+"
    return "other"


def monster_line(seen):
    """seen: species -> set of (max_hp, gen) ints."""
    if not seen:
        return "(none)"
    ranked = sorted(
        seen.items(),
        key=lambda item: (-max(hp for hp, _ in item[1]), int(item[0])),
    )
    parts = []
    for species, pairs in ranked:
        by_hp = collections.defaultdict(set)
        for hp, gen in pairs:
            by_hp[hp].add(gen)
        bits = []
        for hp in sorted(by_hp, reverse=True):
            gens = "+".join(f"g{gen}" for gen in sorted(by_hp[hp]))
            bits.append(f"{hp}/{gens}")
        parts.append(f"{species}:{','.join(bits)}")
    return " ".join(parts)


def perf_phrase(samples, below_limit):
    if not samples:
        return "no samples"
    fps = [row["guest_fps"] for row in samples]
    return (
        f"n={len(samples)} fps mean={mean(fps):.1f} min={min(fps)} "
        f"p5={percentile(fps, 0.05):.1f} "
        f"rpc_req_s mean={mean([row['rpc_req_s'] for row in samples]):.1f} "
        f"pumped mean={mean([row['pumped'] for row in samples]):.1f} "
        f"redraws mean={mean([row['redraws'] for row in samples]):.1f} "
        f"lost_tap sum={sum(row['lost_tap'] for row in samples)} "
        f"below={sum(1 for value in fps if value < below_limit)}"
    )


def summarize(path):
    rows = read_rows(path)
    lines = []
    name = path.rsplit("/", 1)[-1]
    if not rows:
        return [f"== {name} ==", "empty"]

    build = "?"
    for _, rest in rows:
        if rest.startswith("start "):
            fields = kv_parse(rest)
            build = fields.get("build", "?")
            break

    duration = rows[-1][0]
    fs_events = [(0.0, None)]
    fs_state = None
    scene = None
    quests = []
    open_start = None
    tap_states = []
    dmgs = []
    secs = []
    anomalies = {"hp_bad": [], "dmg_drop": [], "tap_unmatched": []}
    scene_times = []
    # Per quest, list of (t, identity) collapsed by identical timestamp.
    quest_groups = []
    current_groups = None

    for t, rest in rows:
        kind = rest.split(None, 1)[0] if rest else ""
        if kind == "azahar":
            nxt = fullscreen_of(rest)
            if nxt != "unchanged" and nxt != fs_state:
                fs_state = nxt
                fs_events.append((t, fs_state))
        elif kind == "scene":
            scene_times.append(t)
            this = scene_name(rest)
            if this != scene:
                if this == "InQuest":
                    open_start = t
                    current_groups = []
                elif scene == "InQuest" and open_start is not None:
                    quests.append(
                        {
                            "start": open_start,
                            "end": t,
                            "open": False,
                            "groups": current_groups or [],
                        }
                    )
                    open_start = None
                    current_groups = None
                scene = this
            if scene == "InQuest" and current_groups is not None:
                ident = monster_identity(rest)
                if current_groups and current_groups[-1][0] == t:
                    current_groups[-1] = (t, ident)
                else:
                    current_groups.append((t, ident))
        elif kind == "tap":
            label = rest.split(None, 1)[1] if " " in rest else rest
            tap_states.append((t, label))
        elif kind == "dmg":
            fields = kv_parse(rest)
            amount = as_int(fields.get("amount"))
            dmgs.append(
                {
                    "t": t,
                    "source": fields.get("source", "?"),
                    "confidence": fields.get("confidence", "?"),
                    "amount": amount,
                    "mon": fields.get("mon", "-"),
                    "species": fields.get("species", "-"),
                    "gen": fields.get("gen", "-"),
                    "max_hp": as_int(fields.get("max_hp")),
                    "hp_before": as_int(fields.get("hp_before")),
                    "hp_after": as_int(fields.get("hp_after")),
                    "frames": fields.get("frames", "-"),
                    "lr": fields.get("lr", "-"),
                    "r3": fields.get("r3", "-"),
                    "sp": fields.get("sp", "-"),
                    "scene": scene,
                }
            )
        elif kind == "sec":
            fields = kv_parse(rest)
            secs.append(
                {
                    "t": t,
                    "scene": scene,
                    "fs": fs_state,
                    "rpc_req_s": as_int(fields.get("rpc_req_s")) or 0,
                    "snaps": as_int(fields.get("snaps")) or 0,
                    "guest_fps": as_int(fields.get("guest_fps")) or 0,
                    "pumped": as_int(fields.get("pumped")) or 0,
                    "redraws": as_int(fields.get("redraws")) or 0,
                    "lost_tap": as_int(fields.get("lost_tap")) or 0,
                    "residual_n": as_int(fields.get("residual_n")) or 0,
                    "residual_sum": as_int(fields.get("residual_sum")) or 0,
                    "tap_n": as_int(fields.get("tap_n")) or 0,
                    "tap_sum": as_int(fields.get("tap_sum")) or 0,
                }
            )
        elif kind in anomalies:
            fields = kv_parse(rest)
            anomalies[kind].append((t, fields, rest))

    if open_start is not None:
        quests.append(
            {
                "start": open_start,
                "end": duration,
                "open": True,
                "groups": current_groups or [],
            }
        )

    final_scene = scene or "?"
    lost_tap_total = sum(row["lost_tap"] for row in secs)
    tap_text = " | ".join(f"{fmt_t(t)} {label}" for t, label in tap_states) or "(none)"

    lines.append(
        f"== {name} == build={build} duration={fmt_t(duration)}s "
        f"lines={len(rows)} quests={len(quests)} final_scene={final_scene}"
    )
    lines.append(f"tap: {tap_text}")
    lines.append(f"lost_tap={lost_tap_total}")

    def in_quest(t, quest):
        if quest["open"]:
            return quest["start"] <= t <= quest["end"]
        return quest["start"] <= t < quest["end"]

    for index, quest in enumerate(quests, start=1):
        start, end = quest["start"], quest["end"]
        windowed, fullscreen, unknown = split_duration(start, end, fs_events)
        seen = collections.defaultdict(set)

        q_dmg = [ev for ev in dmgs if in_quest(ev["t"], quest)]
        q_sec = [row for row in secs if in_quest(row["t"], quest)]
        for ev in q_dmg:
            sp = as_int(ev["species"])
            gen = as_int(ev["gen"])
            hp = ev["max_hp"]
            if sp is None or gen is None or hp is None:
                continue
            seen[str(sp)].add((hp, gen))

        # Scene lines carry max_hp; recover by walking original rows inside the span.
        for t, rest in rows:
            if not rest.startswith("scene ") or not in_quest(t, quest):
                continue
            if scene_name(rest) != "InQuest":
                continue
            tokens = rest.split()
            idx = 0
            while idx < len(tokens):
                if tokens[idx].startswith("slot=") and idx + 5 < len(tokens):
                    fields = {}
                    ok = True
                    for step in range(6):
                        piece = tokens[idx + step]
                        if "=" not in piece:
                            ok = False
                            break
                        key, val = piece.split("=", 1)
                        fields[key] = val
                    if ok and "species" in fields:
                        sp = fields.get("species")
                        hp = as_int(fields.get("max_hp"))
                        gen = as_int(fields.get("gen"))
                        if sp is not None and hp is not None and gen is not None:
                            seen[sp].add((hp, gen))
                    idx += 6
                else:
                    idx += 1

        tap_n = sum(1 for ev in q_dmg if ev["source"] == "tap" and ev["amount"] is not None)
        tap_sum = sum(ev["amount"] for ev in q_dmg if ev["source"] == "tap" and ev["amount"] is not None)
        pas_n = sum(1 for ev in q_dmg if ev["source"] == "passive" and ev["amount"] is not None)
        pas_sum = sum(
            ev["amount"] for ev in q_dmg if ev["source"] == "passive" and ev["amount"] is not None
        )
        total_sum = tap_sum + pas_sum
        sec_tap_n = sum(row["tap_n"] for row in q_sec)
        sec_tap_sum = sum(row["tap_sum"] for row in q_sec)
        sec_res_n = sum(row["residual_n"] for row in q_sec)
        sec_res_sum = sum(row["residual_sum"] for row in q_sec)
        end_label = fmt_t(end) + (" open" if quest["open"] else "")
        unk = f" unknown={unknown:.1f}s" if unknown >= 0.05 else ""
        lines.append(
            f"Q{index} {fmt_t(start)}-{end_label} dur={end - start:.1f}s "
            f"windowed={windowed:.1f}s fullscreen={fullscreen:.1f}s{unk}"
        )
        lines.append(f"  monsters: {monster_line(seen)}")
        lines.append(
            f"  share tap n={tap_n} sum={tap_sum} ({pct(tap_sum, total_sum)}) "
            f"passive n={pas_n} sum={pas_sum} ({pct(pas_sum, total_sum)}) | "
            f"sec tap {sec_tap_n}/{sec_tap_sum} residual {sec_res_n}/{sec_res_sum}"
        )

    by_sc = collections.Counter()
    sums = collections.Counter()
    for ev in dmgs:
        if ev["amount"] is None:
            continue
        key = (ev["source"], ev["confidence"])
        by_sc[key] += 1
        sums[key] += ev["amount"]
    lines.append("damage source+confidence:")
    if not by_sc:
        lines.append("  (none)")
    for key in sorted(by_sc, key=lambda item: (-sums[item], item[0], item[1])):
        lines.append(f"  {key[0]} {key[1]} n={by_sc[key]} sum={sums[key]}")

    ranked = sorted(
        (ev for ev in dmgs if ev["amount"] is not None),
        key=lambda ev: (-ev["amount"], ev["t"]),
    )
    lines.append("top10:")
    if not ranked:
        lines.append("  (none)")
    for ev in ranked[:10]:
        lines.append(
            f"  {fmt_t(ev['t'])} {ev['source']} {ev['confidence']} amt={ev['amount']} "
            f"sp={ev['species']} max={fmt_n(ev['max_hp'])} "
            f"hp={fmt_n(ev['hp_before'])}->{fmt_n(ev['hp_after'])} frames={ev['frames']}"
        )

    over_max = []
    over_before = []
    for ev in dmgs:
        amount = ev["amount"]
        if amount is None:
            continue
        if ev["max_hp"] is not None and amount > ev["max_hp"]:
            over_max.append(ev)
        if ev["hp_before"] is not None and amount > ev["hp_before"]:
            over_before.append(ev)

    def dmg_brief(ev):
        flags = []
        if ev["max_hp"] is not None and ev["amount"] > ev["max_hp"]:
            flags.append("gt_max")
        if ev["hp_after"] == 0:
            flags.append("hp_after=0")
        elif ev["hp_after"] is not None and ev["hp_before"] is not None and ev["hp_after"] != (
            ev["hp_before"] - ev["amount"]
        ):
            flags.append("hp_mismatch")
        flag = (" " + ",".join(flags)) if flags else ""
        return (
            f"  {fmt_t(ev['t'])} {ev['source']} {ev['confidence']} amt={ev['amount']} "
            f"sp={ev['species']} max={fmt_n(ev['max_hp'])} "
            f"hp={fmt_n(ev['hp_before'])}->{fmt_n(ev['hp_after'])} "
            f"frames={ev['frames']}{flag}"
        )

    lines.append(f"amount>max_hp n={len(over_max)}")
    for ev in sorted(over_max, key=lambda ev: ev["t"]):
        lines.append(dmg_brief(ev))
    lines.append(f"amount>hp_before n={len(over_before)}")
    for ev in sorted(over_before, key=lambda ev: ev["t"]):
        lines.append(dmg_brief(ev))

    buckets = collections.Counter()
    passive = [ev for ev in dmgs if ev["source"] == "passive" and ev["amount"] is not None]
    taps_by_mon = collections.defaultdict(list)
    for ev in dmgs:
        if ev["source"] == "tap":
            taps_by_mon[ev["mon"]].append(ev["t"])
    for ev in passive:
        buckets[hist_bucket(ev["amount"])] += 1
    lines.append(
        "passive hist "
        + " ".join(
            f"{label}={buckets[label]}"
            for label in ("1-49", "50-199", "200-499", "500+", "other")
            if label != "other" or buckets[label]
        )
    )
    big = [ev for ev in passive if ev["amount"] >= 200]
    if not big:
        lines.append("passive>=200: none")
    else:
        lines.append(f"passive>=200 n={len(big)}")
        for ev in sorted(big, key=lambda ev: ev["t"]):
            near = any(abs(tt - ev["t"]) <= 1.0 for tt in taps_by_mon.get(ev["mon"], ()))
            lines.append(
                f"  {fmt_t(ev['t'])} amt={ev['amount']} frames={ev['frames']} "
                f"{ev['confidence']} tap_within_1s={'yes' if near else 'no'} mon={ev['mon']}"
            )

    def nearest_scene(t):
        if not scene_times:
            return None
        best = min(scene_times, key=lambda when: abs(when - t))
        return abs(best - t)

    for kind in ("hp_bad", "dmg_drop", "tap_unmatched"):
        items = anomalies[kind]
        reasons = collections.Counter(
            fields.get("reason", "-") for _, fields, _ in items if kind == "hp_bad"
        )
        if kind == "hp_bad" and reasons:
            reason_text = " ".join(f"{key}={count}" for key, count in reasons.most_common())
            lines.append(f"{kind} n={len(items)} reasons {reason_text}")
        else:
            lines.append(f"{kind} n={len(items)}")
        for t, fields, _rest in items[:10]:
            dist = nearest_scene(t)
            near = "no" if dist is None or dist > 2.0 else f"{dist:.3f}s"
            if kind == "hp_bad":
                body = (
                    f"addr={fields.get('addr', '-')} species={fields.get('species', '-')} "
                    f"hp={fields.get('hp', '-')} max_hp={fields.get('max_hp', '-')} "
                    f"reason={fields.get('reason', '-')}"
                )
            elif kind == "dmg_drop":
                body = (
                    f"addr={fields.get('addr', '-')} amount={fields.get('amount', '-')} "
                    f"prev_hp={fields.get('prev_hp', '-')} max_hp={fields.get('max_hp', '-')}"
                )
            else:
                body = f"obj={fields.get('obj', '-')} lr={fields.get('lr', '-')}"
            lines.append(f"  {fmt_t(t)} {body} near_scene={near}")
        if len(items) > 10:
            lines.append(f"  ... {len(items) - 10} more")
    if anomalies["tap_unmatched"]:
        groups = collections.Counter(
            (fields.get("obj", "-"), fields.get("lr", "-"))
            for _, fields, _ in anomalies["tap_unmatched"]
        )
        lines.append("tap_unmatched by obj,lr:")
        for (obj, lr), count in groups.most_common(15):
            lines.append(f"  obj={obj} lr={lr} n={count}")
        if len(groups) > 15:
            lines.append(f"  ... {len(groups) - 15} more groups")

    dmg_times = [ev["t"] for ev in dmgs]
    change_rows = []
    for quest in quests:
        groups = quest["groups"]
        for prev, curr in zip(groups, groups[1:]):
            if curr[1] != prev[1]:
                change_rows.append(curr)
    near_changes = []
    for t, ident in change_rows:
        window = [ev for ev in dmgs if abs(ev["t"] - t) <= 1.0 and ev["amount"] is not None]
        if window:
            near_changes.append((t, ident, window))
    lines.append(
        f"list changes inside quests: {len(change_rows)} "
        f"with dmg within 1s: {len(near_changes)}"
    )
    for t, ident, window in near_changes:
        max_amt = max(ev["amount"] for ev in window)
        lines.append(
            f"  {fmt_t(t)} dmg_n={len(window)} max_amt={max_amt} species={species_brief(ident)}"
        )

    in_quest_secs = []
    for row in secs:
        for quest in quests:
            if in_quest(row["t"], quest):
                in_quest_secs.append(row)
                break
    fps_values = [row["guest_fps"] for row in in_quest_secs]
    typical = mode_of(fps_values)
    if typical is None:
        below_limit = 45
        typical_text = "typical=none"
    else:
        below_limit = max(1, int(round(typical * 0.75)))
        typical_text = f"typical=mode {typical} (below means fps<{below_limit})"
    windowed_s = [row for row in in_quest_secs if row["fs"] is False]
    full_s = [row for row in in_quest_secs if row["fs"] is True]
    unk_s = [row for row in in_quest_secs if row["fs"] is None]
    above = 0
    if typical is not None:
        above = sum(1 for value in fps_values if value > typical * 1.5)
    lines.append(
        f"perf InQuest {typical_text} above_1.5x={above} "
        f"samples windowed={len(windowed_s)} fullscreen={len(full_s)} unknown={len(unk_s)}"
    )
    lines.append(f"  windowed: {perf_phrase(windowed_s, below_limit)}")
    lines.append(f"  fullscreen: {perf_phrase(full_s, below_limit)}")
    if unk_s:
        lines.append(f"  unknown: {perf_phrase(unk_s, below_limit)}")

    # Fullscreen stretches anywhere, so a village-only sample is visible.
    stretches = []
    for index, (when, state) in enumerate(fs_events):
        if state is not True:
            continue
        end_t = duration
        for later, later_state in fs_events[index + 1 :]:
            end_t = later
            break
        if end_t > when:
            stretches.append((when, end_t))
    if stretches:
        for when, end_t in stretches:
            sample = [row for row in secs if when <= row["t"] < end_t]
            scenes = sorted({row["scene"] or "?" for row in sample})
            in_q = sum(
                1
                for row in sample
                if any(in_quest(row["t"], quest) for quest in quests)
            )
            lines.append(
                f"  fullscreen interval {fmt_t(when)}-{fmt_t(end_t)} "
                f"({end_t - when:.1f}s) scenes={','.join(scenes) or '?'} "
                f"inquest_samples={in_q} {perf_phrase(sample, below_limit)}"
            )
    else:
        lines.append("  fullscreen interval: none")

    tap_events = [ev for ev in dmgs if ev["source"] == "tap"]
    lr_counts = collections.Counter(ev["lr"] for ev in tap_events)
    lr_top = " ".join(f"{lr}={count}" for lr, count in lr_counts.most_common(5))
    lines.append(f"tap lr uniq={len(lr_counts)} top: {lr_top or '(none)'}")

    columns = []
    width_counts = collections.Counter()
    r3_counts = collections.Counter()
    sp3_eq = sp4_eq = r3_eq = 0
    for ev in tap_events:
        words = ev["sp"].split(",") if ev["sp"] and ev["sp"] != "-" else []
        width_counts[len(words)] += 1
        while len(columns) < len(words):
            columns.append(collections.Counter())
        for index, word in enumerate(words):
            columns[index][word] += 1
        r3_counts[ev["r3"]] += 1
        mon = as_int(ev["mon"])
        amount = ev["amount"]
        if len(words) >= 5 and amount is not None and as_int(words[3]) == amount:
            sp3_eq += 1
        if len(words) >= 5 and mon is not None and as_int(words[4]) == (mon + 0x100) & 0xFFFFFFFF:
            sp4_eq += 1
        r3 = as_int(ev["r3"])
        if mon is not None and r3 is not None and r3 == (mon - 0x418) & 0xFFFFFFFF:
            r3_eq += 1
    width_text = " ".join(f"{width}w={count}" for width, count in sorted(width_counts.items()))
    lines.append(f"tap sp n={len(tap_events)} {width_text or 'none'}")
    for index, counter in enumerate(columns):
        label = "constant" if len(counter) == 1 else "variable"
        top = " ".join(f"{word}={count}" for word, count in counter.most_common(3))
        lines.append(f"  sp{index} {label} uniq={len(counter)} {top}")
    r3_top = " ".join(f"{word}={count}" for word, count in r3_counts.most_common(5))
    lines.append(f"r3 uniq={len(r3_counts)} top: {r3_top or '(none)'}")
    if tap_events:
        lines.append(
            f"tap derived: r3=mon-0x418 {r3_eq}/{len(tap_events)} "
            f"sp4=mon+0x100 {sp4_eq}/{len(tap_events)} "
            f"sp3==amount {sp3_eq}/{len(tap_events)}"
        )

    holes = 0
    low = []
    for quest in quests:
        q_sec = [row for row in secs if in_quest(row["t"], quest)]
        holes += missing_sec([row["t"] for row in q_sec], quest["start"], quest["end"])
        for row in q_sec:
            if row["snaps"] < 30:
                low.append(row)
    lines.append(f"gaps missing_sec_seconds={holes} snaps<30 n={len(low)}")
    shown = low[:10]
    for row in shown:
        lines.append(
            f"  {fmt_t(row['t'])} snaps={row['snaps']} fps={row['guest_fps']} scene={row['scene']}"
        )
    if len(low) > 10:
        lines.append(f"  ... {len(low) - 10} more")
    return lines


def main(argv):
    if len(argv) < 2:
        print("usage: python3 tools/diag_summary.py LOG [LOG...]", file=sys.stderr)
        return 2
    blocks = []
    for path in argv[1:]:
        blocks.append("\n".join(summarize(path)))
    print("\n\n".join(blocks))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
