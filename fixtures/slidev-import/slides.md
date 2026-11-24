---
theme: seriph
title: The Moth Lantern Census
info: A made-up evening of counting moths, for testing the importer.
canvasWidth: 980
themeConfig:
  primary: '#5d8392'
transition: slide-left
---

# The Moth Lantern Census

A made-up evening of counting moths

<!--
Welcome. Everything in this talk is invented.
-->

---
layout: default
---

# What we brought

<v-clicks>

- A white sheet
- A warm lamp
- A notebook

</v-clicks>

<!-- a comment the presenter never sees -->

Then we waited.

---
layout: two-cols
transition: fade
---

# Two gardens

The north garden had **tall grass**.

<div v-click>Moths came first to the north.</div>

::right::

The south garden had a pond.

<v-click>

And many frogs.

</v-click>

<v-clicks>

- Frogs
- Toads

</v-clicks>

---
layout: section
hide: true
---

# A spare section

---
layout: center
---

# A counter

<Counter :count="10" />

<div class="grid grid-cols-2 gap-4">
<div>Left cell</div>
<div>Right cell</div>
</div>

---
layout: image-right
image: /photo.png
---

# A photograph

![A moth on the sheet](/photo.png)

<div v-after>Taken at midnight.</div>

---
layout: none
---

```ts
const moths = ["luna", "atlas"]
```

<script setup>
const total = 2
</script>

---
src: ./pages/extra.md
---

---
layout: default
transition: view-transition
---

# The end

<v-click at="3">

Shown at the third click.

</v-click>

<!--
Thank everyone for coming.
-->
