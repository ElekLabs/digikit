// Inlined into quality.html. Use two native players so swapping sources does not restart playback.
for (const player of document.querySelectorAll('[data-comparison-player]')) {
  const audios = [...player.querySelectorAll('audio')];
  const buttons = [...player.querySelectorAll('button')];
  const controls = player.querySelector('[role="group"]');
  const status = player.querySelector('[role="status"]');
  let active = 0;
  if (audios.length !== 2) continue;
  controls.hidden = false;

  for (const [index, audio] of audios.entries()) {
    audio.addEventListener('error', () => {
      if (index === active) status.textContent = 'Audio could not load. Check the WAV next to this report.';
    });
    buttons[index].addEventListener('click', () => {
      if (index === active) return;
      const previous = audios[active];
      const time = previous.currentTime;
      const playing = !previous.paused && !previous.ended;
      previous.pause();
      previous.hidden = true;
      buttons[active].setAttribute('aria-pressed', 'false');

      active = index;
      audio.volume = previous.volume;
      audio.muted = previous.muted;
      audio.playbackRate = previous.playbackRate;
      audio.currentTime = Number.isFinite(audio.duration) ? Math.min(time, audio.duration) : time;
      audio.hidden = false;
      buttons[active].setAttribute('aria-pressed', 'true');
      status.textContent = audio.error ? 'Audio could not load. Check the WAV next to this report.' : '';
      if (playing) {
        audio.play().catch(() => {
          if (active === index && audio.paused) status.textContent = 'Playback did not resume. Press Play to continue.';
        });
      }
    });
  }
}
