---
title: Components
layout: title
---

# A talk's own components

The examples from the guide for coders

---

# A score

<Score team="Oslo" points="3" colour="#16a34a" />

<Score team="Bergen" points="1" colour="#dc2626" />

---

# A callout

<Callout title="Remember">

What's written between the tags goes **below the title**.

</Callout>

---

# Three lines, one per click

<Reveal first="Write a component" second="Put its tag on a slide" third="Build it" />

---

# A minute

<Clock size="320" />

---

# Bars, drawn by hand

<Bars data="assets/sales.csv" />

---

# A counter, with buttons

<Counter id="count" x="660" y="300" w="400" h="160" />

<Button label="Add 1" do="add" value="1" on="count" x="560" y="620" w="300" h="96" />

<Button label="Reset" do="reset" on="count" look="secondary" x="1060" y="620" w="300" h="96" />

---

# The same data, as a chart

<Chart kind="bar" data="assets/sales.csv" title="Sales" build="true" />
