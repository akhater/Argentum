# Own RAW tool — feasibility check

**Date:** 2026-09-08
**Question:** Fork RapidRAW, pull darktable's better tools in one at a time. Doable and maintainable?
**Answer: Yes. Green light.**

Checked by reading both codebases. Nothing installed, nothing built yet.

---

## What I was worried about, and what I found

### Worry 1: "Can you even add new tools to RapidRAW without wrecking it?"

**Fine.** Its whole image engine is one file — `src-tauri/src/shaders/shader.wgsl`, 1910 lines.
Inside it, every tool is one self-contained function:

```
apply_white_balance()    apply_sharpen()     apply_dehaze()
apply_color_grading()    apply_local_contrast()   apply_noise_reduction()
```

They run one after another in a list. Adding darktable's version of a tool
= write one more function, add one line to the list. That's it.

It's not a fancy plugin system. But it's a simple, repeatable pattern —
which is better for you than a fancy one.

### Worry 2: "Will darktable's math look wrong once transplanted?"

**Real issue, but solved once.**

The two apps measure color slightly differently (RapidRAW uses one standard,
darktable uses a wider one). Transplanted code needs a small conversion
wrapped around it — about 6 lines of math.

Do it once, reuse it for every tool you bring over afterwards. Not a recurring cost.

---

## Confirmed: darktable's white balance really is better

RapidRAW's actual white balance code, in full:

```wgsl
fn apply_white_balance(color, temp, tint) {
    temp_mult = (1.0 + temp*0.2,  1.0 + temp*0.05,  1.0 - temp*0.2);
    tint_mult = (1.0 + tint*0.25, 1.0 - tint*0.25,  1.0 + tint*0.25);
    return color * temp_mult * tint_mult;
}
```

That's it. Three made-up multiplier numbers. No Kelvin, no camera profile,
no real color science. It's a fudge that looks roughly right.

darktable's does proper chromatic adaptation using the camera's own profile.
Not a small upgrade — a different league. **Good first target.**

---

## What each new tool actually costs you

Four files, same four every time:

| File | What you add |
|---|---|
| `shader.wgsl` | the math |
| `image_processing.rs` | the settings it takes |
| a `.tsx` file | the slider in the UI |
| translation file | the label text |

**First one: several days** (learning the pattern).
**After that: 1–2 days each.**

That's lower than my earlier guess of 1–2 weeks. Reading the code made it look easier, not harder.

---

## Will it stay maintainable?

Yes, with one caveat.

You pull RapidRAW's updates with `git pull` and they mostly just land.
The caveat: you *are* editing their main shader file, so occasionally an
update touches the same file. But since your changes are functions added at
the bottom plus one line in the list, those clashes are quick to sort out.
Not the merge hell that kills forks.

darktable updates don't flow in — but that math doesn't change year to year.
Bring a tool over once and it's done.

---

## The recipe problem — solved for free

Right now you can't start an edit in one app and finish in the other because
each app's edit file is a list of *its own* tools. Neither can run the other's.

One app that has both sets of tools = one list that covers everything.
Problem disappears. Not extra work — it's automatic once they're in the same app.

---

## Suggested first move

Port darktable's white balance. It's the one you named, it's the biggest
visible win, and it forces you to build the color conversion that every
later tool reuses.

If that lands, everything after it is the same job, easier.

---

## Follow-up: what about darktable's updates?

They don't flow in automatically. But measured against reality, that's a minor cost.

darktable's **math** is near-frozen. Its **app code** churns constantly.
Commits in the last 3 years:

| File | Commits |
|---|---|
| `data/kernels/filmic.cl` — the math you'd copy | **10** |
| `src/iop/filmicrgb.c` — GUI/plumbing around it | 139 |
| `src/iop/channelmixerrgb.c` — white balance module | 176 |

Roughly 14x more churn in the app code than the math.

I read all 10 filmic math commits. Every one is housekeeping:
copyright headers, OpenCL helper macros, a performance flag, a rounding constant.
**None changed the visual result.**

**So:** copy a tool over and it stays current for years. Re-check maybe once a year.

**The one real cost:** if darktable ships a genuinely *new* tool you want,
you go fetch it by hand — about a day, and only when you decide you want it.
