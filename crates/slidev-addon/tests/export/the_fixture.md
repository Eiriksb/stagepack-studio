---
theme: default
title: "A made-up talk"
canvasWidth: 1920
aspectRatio: 16/9
layout: cover
transition: fade
---

<style>
:global(:root) {
  --slidev-theme-primary: #0d9488;
}
:global(.slidev-layout) {
  background: #f8fafc;
  color: #0f172a;
  font-family: "Inter", sans-serif;
  font-size: 40px;
}
:global(.slidev-layout h1) {
  font-size: 64px;
}
:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {
  font-size: 96px;
}
:global(.slidev-layout code), :global(.slidev-layout pre) {
  font-family: "JetBrains Mono", monospace;
}
</style>

# Notes from the Lantern Reef Survey

A made-up season of counting tide pools

<!--
Welcome everyone. This talk is invented for testing.
-->

---
layout: default
transition: slide-left
---

# What we counted

<v-clicks>

- Starfish, by colour
- Anemones, open and closed
- Hermit crabs that changed shells

</v-clicks>

---
layout: two-cols
transition: none
---

# Two transects

On the left, the **north** transect, with *calm* water.

::right::

<div style="position:absolute;left:1000px;top:260px;width:780px;height:600px;transform:rotate(3deg)">

On the right, the south transect, with <span style="color:#b91c1c;font-weight:bold">strong surf</span>.

</div>

---
layout: section
transition: none
---

# Part two: the data

What the tide left behind

---
layout: none
transition: none
---

<div style="position:absolute;left:560px;top:400px;width:800px;height:280px;background:#fef3c7;border:4px solid #d97706;border-radius:32px;padding:24px;box-sizing:border-box" v-click="1">

A quiet slide, with one box.

</div>

<div style="position:absolute;left:560px;top:720px;width:800px;height:80px;font-size:36px;font-style:italic">

Then a line of its own, set in italics.

</div>

---
layout: default
transition: none
---

# Every shape

<div style="position:absolute;left:100px;top:260px;width:260px;height:180px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate">
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><rect x="0" y="0" width="260" height="180" fill="#ffa500" stroke="#000000" stroke-width="4"/></svg>
</div>

<div style="position:absolute;left:400px;top:260px;width:260px;height:180px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate">
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><path d="M24,0 L236,0 Q260,0 260,24 L260,156 Q260,180 236,180 L24,180 Q0,180 0,156 L0,24 Q0,0 24,0 Z" fill="#dbeafe"/></svg>
</div>

<div style="position:absolute;left:700px;top:260px;width:260px;height:180px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate">
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><ellipse cx="130" cy="90" rx="130" ry="90" fill="#dcfce7" stroke="#15803d" stroke-width="1"/></svg>
</div>

<div style="position:absolute;left:1000px;top:260px;width:260px;height:180px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate">
<svg width="260" height="180" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><line x1="0" y1="0" x2="260" y2="180" stroke="#334155" stroke-width="6"/></svg>
</div>

<div style="position:absolute;left:1300px;top:350px;width:260px;height:0px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate" v-motion :initial="{y:40,opacity:0}" :click-2="{y:0,opacity:1}">
<svg width="260" height="0" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><defs><marker id="arrow-1" markerUnits="userSpaceOnUse" markerWidth="24" markerHeight="24" refX="0" refY="12" orient="auto"><path d="M0,0 L24,12 L0,24 Z" fill="#16a34a"/></marker></defs><line x1="0" y1="0" x2="236" y2="0" stroke="#16a34a" stroke-width="4" marker-end="url(#arrow-1)"/></svg>
</div>

<div style="position:absolute;left:100px;top:560px;width:520px;height:240px;display:flex;flex-direction:column;justify-content:center;align-items:center;isolation:isolate;padding-bottom:40px;box-sizing:border-box">
<svg width="520" height="240" style="position:absolute;left:0;top:0;overflow:visible;z-index:-1"><path d="M24,0 L496,0 Q520,0 520,24 L520,176 Q520,200 496,200 L182,200 L78,240 L104,200 L24,200 Q0,200 0,176 L0,24 Q0,0 24,0 Z" fill="#fae8ff" stroke="#a21caf" stroke-width="1"/></svg>

Shapes can hold words too.

</div>

---
layout: default
transition: none
clicks: 1
---

# An icon, a picture and a gauge

<img style="position:absolute;left:100px;top:840px;width:96px;height:96px" src="/baked-7-0.png" alt="">

<img style="position:absolute;left:1320px;top:560px;width:480px;height:270px" src="/photo.png" alt="">

<div>
<img style="position:absolute;left:700px;top:560px;width:400px;height:300px" v-if="$clicks === 0" src="/baked-7-1-0.png" alt="">
<img style="position:absolute;left:700px;top:560px;width:400px;height:300px" v-if="$clicks >= 1" src="/baked-7-1-1.png" alt="">
</div>

---
layout: default
transition: none
---

# Counts by pool

| Pool | Starfish | Anemones |
| --- | --- | --- |
| A | 12 | 30 |
| B | 7 | 41 |

---
layout: default
transition: none
---

# The counting script

```python
counts = {"starfish": 12, "anemones": 30}
for name, count in counts.items():
    print(f"{name}: {count}")
```

---
layout: default
transition: none
hide: true
---

# Spare slide

Kept for questions, hidden in the talk.

<!--
Only if someone asks --&gt; about the weather.
-->
