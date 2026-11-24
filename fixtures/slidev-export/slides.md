---
title: Notes from the Lantern Reef Survey
layout: title
color.background: #f8fafc
color.text: #0f172a
color.accent: #0d9488
font: Inter
transition: fade
---

# Notes from the Lantern Reef Survey

A made-up season of counting tide pools

<Notes>
Welcome everyone. This talk is invented for testing.
</Notes>

---
layout: content
transition: push
region.body: click=each
---

# What we counted

- Starfish, by colour
- Anemones, open and closed
- Hermit crabs that changed shells

---
layout: two-columns
region.right: x=1000 y=260 w=780 h=600 rotate=3
---

# Two transects

On the left, the **north** transect, with *calm* water.

<Region name="right" />

On the right, the south transect, with <Text color="#b91c1c" bold>strong surf</Text>.

---
layout: section
transition: none
---

# Part two: the data

What the tide left behind

---
layout: blank
---

<Box x="560" y="400" w="800" h="280" fill="#fef3c7" outline="#d97706" outline-width="4" radius="32" click="1" enter="fade">

A quiet slide, with one box.

</Box>

<Text x="560" y="720" w="800" h="80" size="36" italic>

Then a line of its own, set in italics.

</Text>

---

# Every shape

<Shape kind="rect" x="100" y="260" w="260" h="180" fill="orange" outline="black" outline-width="4" />

<Shape kind="rounded" x="400" y="260" w="260" h="180" fill="#dbeafe" />

<Shape kind="ellipse" x="700" y="260" w="260" h="180" fill="#dcfce7" outline="#15803d" />

<Shape kind="line" x="1000" y="260" w="260" h="180" outline="#334155" outline-width="6" />

<Shape kind="arrow" x="1300" y="350" w="260" h="0" fill="#16a34a" click="2" enter="fly-up" />

<Shape kind="callout" x="100" y="560" w="520" h="240" fill="#fae8ff" outline="#a21caf">

Shapes can hold words too.

</Shape>

---

# An icon, a picture and a gauge

<Icon name="arrow-right" x="100" y="840" size="96" />

<Picture src="assets/photo.png" x="1320" y="560" w="480" h="270" />

<TideGauge level="3" x="700" y="560" w="400" h="300" />

---

# Counts by pool

| Pool | Starfish | Anemones |
| --- | --- | --- |
| A | 12 | 30 |
| B | 7 | 41 |

---

# The counting script

```python
counts = {"starfish": 12, "anemones": 30}
for name, count in counts.items():
    print(f"{name}: {count}")
```

---
hidden: true
---

# Spare slide

Kept for questions, hidden in the talk.

<Notes>
Only if someone asks --> about the weather.
</Notes>
