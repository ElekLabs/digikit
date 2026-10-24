# Bass Drum

A kick drum you build from the machine page instead of from a sample: a sine with its own pitch envelope, a short
noise click, and a drive stage. The **amplitude** decay is the track's AMP envelope, so shape the body and the tail
there — the machine only has to sound right for the first 50 ms.

| knob | range |
|---|---|
| **TUNE** | the note (stock knob, slot 0) — the kick's fundamental |
| **BEND** | how far the pitch starts above the note; full is four times the frequency at the strike, falling to the note over about 58 ms |
| **CLICK** | a noise transient over the first ~6 ms |
| **DRIVE** | gain into a clamp: rounds the peaks rather than clipping them |

**Turn the AMP decay down** to between 20 and 40 for a kick, and put a little FLTR envelope on it if you want the
click to bite. BEND at ¾ with CLICK at ¼ is a good starting point; BEND lower and longer decay is a sub.

The machine generates its own note-on: the phase and a block counter are zeroed at every trigger, and the counter is
what drives both envelopes. That is why it responds to the first trig after power-on without any state of its own.
