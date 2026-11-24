---
title: From the first click
layout: title
transition: none
region.body: click=each enter=fade
---

# From the first click

- One
- Two
- Three

---
layout: content
transition: none
---

# After a click

<Box click="1" enter="fade">

First

</Box>

<Box click="2" enter="fade">

1. Two

</Box>

<Box click="3" enter="fade">

2. Three

</Box>

---
layout: content
transition: none
---

# At five, two at a time

<Box click="5" enter="fade">

- A

</Box>

<Box click="5" enter="fade">

- B

</Box>

<Box click="6" enter="fade">

- C

</Box>

---
layout: content
transition: none
region.body: click=each enter=fade
---

# Paragraphs, from the first click

One

Two

---
layout: content
transition: none
---

# A list with words around it

Before the list.

<Box click="1" enter="fade">

- One

</Box>

<Box click="2" enter="fade">

- Two

</Box>
