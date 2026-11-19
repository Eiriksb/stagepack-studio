# Plan G2: decisions made while building

The calls made during plan G2 without asking the owner, and the smaller findings and notes left for later. Each decision says what it costs if it's wrong. `docs/checks/plan-g2.md` has what ran.

## Questions for the owner

- **Smoothness hasn't been seen on a real screen.** The nested display shows 7 to 15 pictures a second, so how smoothly a model turns is a hand check, on a discrete card, an integrated card and Windows (`docs/checks/plan-g2.md`).
- **Nothing was run on Windows.** The drawer there uses DirectX 12, and the window DirectX too.
- **A model is drawn on the best card there is**, which on a laptop may wake the discrete card and use its battery.
- **PowerPoint and PDF get a model's still**, with one sentence saying how many models became pictures.

## Rulings

- **Setup.** The plan wasn't held for the owner's review (the owner said to go ahead with all five pieces, then packaging). Cost if wrong: rework.
- **Task 1.** The facts cache stays one cache; Task 5 added the model's entry. A model kept compressed in a pack is copied unchecked and refused by the reader if it isn't one.
- **Task 2.** Only a model's first base-colour picture is drawn; parts using another are drawn in their material's colour. A point cloud's header facts give no box, so a cloud is framed from a whole read. Every texture is held to 300 MB while it's decoded, checked again after the decoder reads its header; a JPEG cut short is damaged, as a video is. A very large CMYK texture takes about 2.5 s to read in a test build.
- **Task 3.** A model isn't kept on the processor once it's on the card: after a lost device it's read again from its file. A refusal by the card for a model within the limits says so in its own words. Every error the card reports inside a draw is that refusal, not a lost device.
- **Task 4.** A model's views are its slide's steps: keys, the remote and clicks fly it from one to the next, and going back flies back. Cost: a talk can't fly a model apart from its steps. A model lost from the card twice in a row with no picture between can't be drawn. The window-never-waits test asks for 20 frames in its 2 s, not 50, under a loaded machine.
- **Task 5.** A file that no computer could draw (not a model, damaged, too large, needing other files) is refused before it's copied; one this computer can't draw is added. The rotation field is *Rotation* on a model's form, beside *Turns on its own*. A written view keeps four figures of its distance's scale.
- **Final.** One fix wave took both Important findings, the five small ones, Task 6's four small ones and a security check's cap on unpacking. A compressed model that lies about its size says it couldn't be read.

## Smaller findings and notes

- **Fixed by the reviews, before the end:** a crafted model file could make the reader allocate gigabytes from a few hundred kilobytes; a damaged or crafted texture could take nearly 500 MB; a JPEG's headers could be read one way for its budget and another by its decoder; any error from the card restarted it for ever and said the computer couldn't draw models; a failed restart was for good; a model of one point drew nothing; a model that couldn't be drawn kept the show's window redrawing; Cancel part-way through a point cloud's read could loop for ever; a model a pack holds compressed showed a panel for ever; the drawer could be started after quitting.
- **Not fixed:** a dropped model goes in the middle of the slide, not where it was dropped; the first live picture takes 2 to 3 s after *Turn* with nothing to say it's coming; in the headless test window a drop after any key press doesn't arrive (unexplained, the tests work around it); a model refused by the card at one size is asked for again after a resize; the one-sample drawing test can't bite on a card with only one sample; a repeating refusal is written to the log once a frame; reading a slide's models one at a time means a slow one holds up the next; the test made for the unpacking cap passes without it, since the pack's own checks bound it first.
- **Disk:** the build cache was cleared of its incremental files three times during the plan.
