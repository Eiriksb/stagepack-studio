---
title: Show
layout: title
---

# A talk to present

Three slides, with clicks, entrances and notes

<Box x="560" y="780" w="800" h="140" fill="#fef3c7" outline="#d97706" radius="28" click="1" enter="fade">

Appears on the first click, fading in

</Box>

<Notes>
Welcome everyone. The yellow box fades in on the first click.
</Notes>

---
layout: content
transition: fade
region.body: click=each enter=fly-up
---

# Points that fly up

- First point
- Second point
- Third point

<Notes>
One point per click. Pause after each. This slide came in with a fade.
</Notes>

---
layout: section
transition: push
---

# The last slide

Pushed in from the right

<Notes>
Thank them and take questions.
</Notes>
