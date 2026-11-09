---
title: Showcase
layout: title
---

# Stagepack Showcase

A tour of every layout and built-in

---
layout: content
---

# The content layout

- A title, then a body
- Lists, with **bold**, *italic* and `code`
- Plenty of room to talk

---
layout: two-columns
---

# Two columns

On the left, some words about the first idea.

<Region name="right" />

On the right, some words about the second idea.

---
layout: section
---

# A new section

Where the talk turns

---
layout: blank
---

<Box x="560" y="400" w="800" h="280" fill="#fef3c7" outline="#d97706" radius="32">

Just a few words, with nothing around them.

</Box>

---

# Shapes, an icon, a picture and a table

<Box x="100" y="560" w="520" h="200" fill="#dbeafe" outline="#1d4ed8" radius="24">

In a box

</Box>

<Shape kind="ellipse" x="700" y="560" w="240" h="200" fill="orange" outline="black" outline-width="4" />

<Shape kind="arrow" x="1000" y="640" w="240" h="0" fill="#16a34a" />

<Icon name="arrow-right" x="100" y="840" size="96" />

<Picture src="assets/photo.png" x="1320" y="560" w="480" h="270" />

| Layout | Regions |
| --- | --- |
| title | title, subtitle |
| content | title, body |

---

# Code

```rust
fn main() {
    let talk = "saved as Markdown";
    println!("{talk}");
}
```
