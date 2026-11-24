---
title: A made-up talk
color.background: #f8fafc
color.text: #0f172a
color.accent: #0d9488
font: Inter
font.mono: JetBrains Mono
layout: title
transition: none
---

# Notes from the Lantern Reef Survey

A made-up season of counting tide pools

<Notes>
Welcome everyone. This talk is invented for testing.
</Notes>

---
layout: content
transition: push
region.body: click=each enter=fade
---

# What we counted

- Starfish, by colour
- Anemones, open and closed
- Hermit crabs that changed shells

---
layout: two-columns
transition: none
---

# Two transects

On the left, the **north** transect, with *calm* water.

<Region name="right" />

<Box x="1000" y="260" w="780" h="600" rotate="3">

On the right, the south transect, with <Text color="#b91c1c" bold>strong surf</Text>.

</Box>

---
layout: section
transition: none
---

# Part two: the data

What the tide left behind

---
layout: blank
transition: none
---

<Box x="560" y="400" w="800" h="280" fill="#fef3c7" outline="#d97706" outline-width="4" radius="32" padding="24" click="1" enter="fade">

A quiet slide, with one box.

</Box>

<Text x="560" y="720" w="800" h="80" size="36" italic>

Then a line of its own, set in italics.

</Text>

---
layout: content
transition: none
---

# Every shape

<Box x="100" y="260" w="260" h="180">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><rect x="0" y="0" width="260" height="180" fill="#ffa500" stroke="#000000" stroke-width="4"/></svg>
```

</Box>

</Box>

<Box x="400" y="260" w="260" h="180">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><path d="M24,0 L236,0 Q260,0 260,24 L260,156 Q260,180 236,180 L24,180 Q0,180 0,156 L0,24 Q0,0 24,0 Z" fill="#dbeafe"/></svg>
```

</Box>

</Box>

<Box x="700" y="260" w="260" h="180">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><ellipse cx="130" cy="90" rx="130" ry="90" fill="#dcfce7" stroke="#15803d" stroke-width="1"/></svg>
```

</Box>

</Box>

<Box x="1000" y="260" w="260" h="180">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><line x1="0" y1="0" x2="260" y2="180" stroke="#334155" stroke-width="6"/></svg>
```

</Box>

</Box>

<Box x="1300" y="350" w="260" h="0" click="2" enter="fly-up">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="260" height="0" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><defs><marker id="arrow-1" markerUnits="userSpaceOnUse" markerWidth="24" markerHeight="24" refX="0" refY="12" orient="auto"><path d="M0,0 L24,12 L0,24 Z" fill="#16a34a"/></marker></defs><line x1="0" y1="0" x2="236" y2="0" stroke="#16a34a" stroke-width="4" marker-end="url(#arrow-1)"/></svg>
```

</Box>

</Box>

<Box x="100" y="560" w="520" h="240">

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element svg**

```text
<svg width="520" height="240" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><path d="M24,0 L496,0 Q520,0 520,24 L520,176 Q520,200 496,200 L182,200 L78,240 L104,200 L24,200 Q0,200 0,176 L0,24 Q0,0 24,0 Z" fill="#fae8ff" stroke="#a21caf" stroke-width="1"/></svg>
```

</Box>

Shapes can hold words too.

</Box>

---
layout: content
transition: none
---

# An icon, a picture and a gauge

<Picture src="assets/stagepack-baked/7-0.png" x="100" y="840" w="96" h="96" />

<Picture src="assets/photo.png" x="1320" y="560" w="480" h="270" />

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element img**

```text
<img style="position:absolute;left:700px;top:560px;width:400px;height:300px" v-if="$clicks === 0" src="/stagepack-baked/7-1-0.png" alt="">
```

</Box>

<Box outline="#94a3b8" outline-width="2" radius="8" padding="16">

**From Slidev, not converted: the HTML element img**

```text
<img style="position:absolute;left:700px;top:560px;width:400px;height:300px" v-if="$clicks >= 1" src="/stagepack-baked/7-1-1.png" alt="">
```

</Box>

---
layout: content
transition: none
---

# Counts by pool

| Pool | Starfish | Anemones |
| --- | --- | --- |
| A | 12 | 30 |
| B | 7 | 41 |

---
layout: content
transition: none
---

# The counting script

```python
counts = {"starfish": 12, "anemones": 30}
for name, count in counts.items():
    print(f"{name}: {count}")
```

---
layout: content
transition: none
hidden: true
---

# Spare slide

Kept for questions, hidden in the talk.

<Notes>
Only if someone asks --> about the weather.
</Notes>
