/*
 * Bazzica — effetti visivi e sonori della partita.
 *
 * Port del vecchio client web (bazzica/web/js: audio.js, anim.js e il "regista" degli eventi
 * di game.js) in un unico script classico, senza import e senza framework.
 *
 * API (window.BazzicaFx):
 *   istantanea()        memorizza i rettangoli di [data-carta] e [data-seat] PRIMA del nuovo render
 *   eventi(lista, ctx)  accoda e anima una lista di eventi (dopo il render), in ordine; ritorna una promise
 *   suona(nome), setMuto(bool), isMuto(), tic(), turno(), sblocca(), toast(testo, ms)
 *
 * ctx = { io, miei, nomi, squadre, mazzo: "originale" | "piacentine" }
 *
 * Approccio FLIP: quando gli effetti partono il nuovo stato è già nel DOM. Le carte nuove vengono
 * nascoste finché il loro volo non arriva, quelle sparite (prese) restano come "fantasmi" nel
 * livello #fx nella posizione memorizzata da istantanea(), e quelle rimaste ma spostate vengono
 * riportate dov'erano e poi fatte scivolare al nuovo posto.
 */
(function () {
  "use strict";
  if (window.BazzicaFx) return;

  // =================================================================== utilità
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const dopoFrame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

  function el(tag, attrs = {}, ...children) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v === undefined || v === null || v === false) continue;
      if (k === "class") e.className = v;
      else if (k === "html") e.innerHTML = v;
      else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
      else if (k === "dataset") Object.assign(e.dataset, v);
      else e.setAttribute(k, v === true ? "" : v);
    }
    for (const ch of children.flat()) {
      if (ch === null || ch === undefined || ch === false) continue;
      e.append(ch instanceof Node ? ch : document.createTextNode(String(ch)));
    }
    return e;
  }

  const POSTI = ["Nord", "Est", "Sud", "Ovest"];
  const SEMI = ["denari", "coppe", "spade", "bastoni"];
  const NOMI_VALORE = { 1: "Asso", 2: "Due", 3: "Tre", 4: "Quattro", 5: "Cinque", 6: "Sei", 7: "Sette", 8: "Fante", 9: "Cavallo", 10: "Re" };
  const NOMI_TIPO = { tris: "Tris", coppia: "Coppia con somma ≤ 9", diverse: "Tre carte diverse con somma ≤ 9" };
  const val = (c) => parseInt(String(c).slice(-2), 10);
  const seme = (c) => String(c).split("_")[0];
  const squadra = (s) => s % 2;
  function nomeCarta(c) {
    if (c === "denari_07") return "Sette bello";
    if (c === "denari_10") return "Re bello";
    return `${NOMI_VALORE[val(c)]} di ${seme(c)}`;
  }
  const pesca = (a) => a[Math.floor(Math.random() * a.length)];
  const ridotto = () => { try { return window.matchMedia("(prefers-reduced-motion: reduce)").matches; } catch (e) { return false; } };
  function memoria(k, v) {
    try {
      if (v === undefined) return localStorage.getItem(k);
      localStorage.setItem(k, v);
    } catch (e) { /* storage non disponibile */ }
    return null;
  }

  // =================================================================== AUDIO (da audio.js)
  // Suoni sintetizzati con Web Audio API; per alcuni nomi esiste un file registrato in suoni/.
  let actx = null;
  let muto = memoria("bazzica.muto") === "1";
  const FILE = {};
  const PRECARICATI = {};

  function impostaFileSuoni(nomi) {
    for (const n of nomi || []) {
      const base = n.replace(/\.[^.]+$/, "");
      FILE[base] = "suoni/" + n;
      // precarica, così il suono parte senza ritardo la prima volta
      try {
        const a = new Audio(FILE[base]);
        a.preload = "auto";
        // file mancante o illeggibile: si torna alla sintesi
        a.addEventListener("error", () => { delete FILE[base]; delete PRECARICATI[base]; }, { once: true });
        a.load();
        PRECARICATI[base] = a;
      } catch (e) { /* niente audio */ }
    }
  }
  impostaFileSuoni(["bazzica2.mp3", "bazzica3.mp3", "bazzica5.mp3", "bazzica7.mp3", "scopa.mp3"]);

  const isMuto = () => muto;
  function setMuto(v) { muto = !!v; memoria("bazzica.muto", muto ? "1" : "0"); }

  function ac() {
    if (!actx) {
      const C = window.AudioContext || window.webkitAudioContext;
      if (!C) return null;
      actx = new C();
    }
    if (actx.state === "suspended") { try { const p = actx.resume(); if (p && p.catch) p.catch(() => {}); } catch (e) { /* ignora */ } }
    return actx;
  }
  // uscita comune con compressore: le folle sommano molte voci e non devono distorcere
  let master = null;
  function uscita() {
    const a = ac(); if (!a) return null;
    if (!master) {
      master = a.createDynamicsCompressor();
      master.threshold.value = -14; master.knee.value = 12; master.ratio.value = 6;
      master.attack.value = 0.005; master.release.value = 0.25;
      master.connect(a.destination);
    }
    return master;
  }
  function sblocca() { try { ac(); } catch (e) { /* audio non disponibile */ } }
  // sblocca l'audio al primo gesto
  window.addEventListener("pointerdown", sblocca, { once: true });

  function tono(freq, t0, dur, { tipo = "sine", vol = 0.2, slide = 0 } = {}) {
    const a = ac(); if (!a) return;
    const o = a.createOscillator(), g = a.createGain();
    o.type = tipo;
    o.frequency.setValueAtTime(freq, a.currentTime + t0);
    if (slide) o.frequency.exponentialRampToValueAtTime(Math.max(30, freq * slide), a.currentTime + t0 + dur);
    g.gain.setValueAtTime(0.0001, a.currentTime + t0);
    g.gain.exponentialRampToValueAtTime(vol, a.currentTime + t0 + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, a.currentTime + t0 + dur);
    o.connect(g).connect(uscita());
    o.start(a.currentTime + t0);
    o.stop(a.currentTime + t0 + dur + 0.05);
  }

  function rumore(t0, dur, { vol = 0.25, freq = 2000, q = 1, tipo = "bandpass", sweep = 0 } = {}) {
    const a = ac(); if (!a) return;
    const n = Math.floor(a.sampleRate * dur);
    const buf = a.createBuffer(1, n, a.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = (Math.random() * 2 - 1) * (1 - i / n);
    const s = a.createBufferSource(); s.buffer = buf;
    const f = a.createBiquadFilter(); f.type = tipo; f.Q.value = q;
    f.frequency.setValueAtTime(freq, a.currentTime + t0);
    if (sweep) f.frequency.exponentialRampToValueAtTime(freq * sweep, a.currentTime + t0 + dur);
    const g = a.createGain(); g.gain.value = vol;
    s.connect(f).connect(g).connect(uscita());
    s.start(a.currentTime + t0);
  }

  // ----------------------------------------------------------------- folla da stadio
  // Formanti (F1, F2) delle vocali cantate.
  const VOCALI = { a: [730, 1090], e: [530, 1840], i: [300, 2200], o: [470, 820], u: [320, 800] };

  /** Una voce cantata: dente di sega con vibrato e leggero glissando, filtrata da due formanti. */
  function voce(t0, durata, freq, vocale, { vol = 0.05, glide = 1, attacco = 0.06 } = {}) {
    const a = ac(); if (!a) return;
    const T = a.currentTime + t0;
    const o = a.createOscillator(); o.type = "sawtooth";
    o.frequency.setValueAtTime(freq, T);
    if (glide !== 1) o.frequency.exponentialRampToValueAtTime(freq * glide, T + durata);
    const lfo = a.createOscillator(), lg = a.createGain();
    lfo.frequency.value = 4.5 + Math.random() * 2; lg.gain.value = freq * 0.012;
    lfo.connect(lg).connect(o.frequency);
    const g = a.createGain();
    g.gain.setValueAtTime(0.0001, T);
    g.gain.linearRampToValueAtTime(vol, T + attacco + Math.random() * 0.05);
    g.gain.setValueAtTime(vol, T + Math.max(attacco, durata - 0.12));
    g.gain.linearRampToValueAtTime(0.0001, T + durata);
    const [f1, f2] = VOCALI[vocale];
    for (const [f, q, k] of [[f1, 6, 1], [f2, 8, 0.6]]) {
      const bp = a.createBiquadFilter(); bp.type = "bandpass"; bp.frequency.value = f; bp.Q.value = q;
      const kg = a.createGain(); kg.gain.value = k;
      o.connect(bp).connect(kg).connect(g);
    }
    g.connect(uscita());
    o.start(T); lfo.start(T);
    o.stop(T + durata + 0.05); lfo.stop(T + durata + 0.05);
  }

  /** Un coro di tante voci leggermente stonate e sfasate, come una curva di stadio. */
  function coro(t0, durata, freq, vocale, { voci = 16, vol = 0.13, glide = 1, sparpaglia = 0.08 } = {}) {
    for (let i = 0; i < voci; i++) {
      const ottava = i % 5 === 0 ? 0.5 : 1;                       // qualche voce più grave
      const stonato = freq * ottava * (1 + (Math.random() - 0.5) * 0.035);
      voce(t0 + Math.random() * sparpaglia, durata * (0.9 + Math.random() * 0.2), stonato, vocale, { vol, glide: glide * (1 + (Math.random() - 0.5) * 0.04) });
    }
  }

  /** Brusio/boato di folla: rumore con modulazioni lente e casuali (tante bocche insieme). */
  function boato(t0, durata, { vol = 0.6, picco = 0.25, freq = 1000 } = {}) {
    const a = ac(); if (!a) return;
    const n = Math.floor(a.sampleRate * durata);
    const buf = a.createBuffer(2, n, a.sampleRate);
    for (let ch = 0; ch < 2; ch++) {
      const d = buf.getChannelData(ch);
      let mod = 0.6, obiettivo = 0.6;
      for (let i = 0; i < n; i++) {
        if (i % 2200 === 0) obiettivo = 0.35 + Math.random() * 0.65;
        mod += (obiettivo - mod) * 0.0008;
        const t = i / n;
        const inv = t < picco ? t / picco : 1 - (t - picco) / (1 - picco);
        d[i] = (Math.random() * 2 - 1) * mod * Math.pow(Math.max(inv, 0), 1.3);
      }
    }
    const src = a.createBufferSource(); src.buffer = buf;
    const bp = a.createBiquadFilter(); bp.type = "bandpass"; bp.frequency.value = freq; bp.Q.value = 0.6;
    const ls = a.createBiquadFilter(); ls.type = "lowshelf"; ls.frequency.value = 300; ls.gain.value = 6;
    const g = a.createGain(); g.gain.value = vol;
    src.connect(bp).connect(ls).connect(g).connect(uscita());
    src.start(a.currentTime + t0);
  }

  /** "Ooooh!" di stupore: tante voci che salgono e poi scendono sulla vocale O. */
  function ooh(t0, durata = 1.6, vol = 0.16, voci = 18) {
    for (let i = 0; i < voci; i++) {
      const f = 170 + Math.random() * 190;
      const T = t0 + Math.random() * 0.12;
      voce(T, durata * 0.4, f, "o", { vol, glide: 1.18 });
      voce(T + durata * 0.38, durata * 0.62, f * 1.18, "o", { vol, glide: 0.62 });
    }
    boato(t0, durata, { vol: 0.1, picco: 0.35, freq: 700 });
  }

  /** Applausi: molti battiti di mani brevi e casuali. */
  function applausi(t0, durata, densita = 60, vol = 0.22) {
    const n = Math.floor(durata * densita);
    for (let i = 0; i < n; i++) {
      const t = t0 + Math.random() * durata;
      const sfuma = 1 - (t - t0) / durata * 0.6;
      rumore(t, 0.025 + Math.random() * 0.02, { vol: vol * sfuma * (0.5 + Math.random() * 0.5), freq: 1200 + Math.random() * 1800, q: 1.5 });
    }
  }

  /** Battimani ritmati della curva: clap-clap, clap-clap-clap. */
  function battimani(t0, ripetizioni = 1, vol = 0.5) {
    const schema = [0, 0.28, 0.75, 1.0, 1.25];
    for (let r = 0; r < ripetizioni; r++) {
      for (const s0 of schema) {
        for (let k = 0; k < 18; k++) rumore(t0 + r * 1.7 + s0 + Math.random() * 0.03, 0.04, { vol: vol / 6, freq: 1500 + Math.random() * 1200, q: 1.2 });
      }
    }
  }

  /** Coro "Baz-zi-caaa!" della curva (melodia originale: tre note, l'ultima tenuta). */
  function coroBazzica(t0, volte = 1, voci = 20) {
    const nota = [392, 392, 523.25];
    for (let k = 0; k < volte; k++) {
      const t = t0 + k * 1.7;
      coro(t, 0.24, nota[0], "a", { voci });
      coro(t + 0.28, 0.24, nota[1], "i", { voci });
      coro(t + 0.56, 0.9, nota[2], "a", { voci, glide: 1.03 });
    }
  }

  /** Trombetta da stadio. */
  function trombetta(t0, durata = 0.8, vol = 0.12) {
    [466, 554, 698].forEach((f) => tono(f, t0, durata, { tipo: "sawtooth", vol: vol / 3 }));
  }

  // ----------------------------------------------------------------- spazzola, campana, ottoni
  /** Un colpo di spazzola sul panno: rumore che passa da grave ad acuto, con le setole che crepitano. */
  function spazzolata(t0, durata, forza = 1) {
    const a = ac(); if (!a) return;
    const T = a.currentTime + t0;
    const n = Math.floor(a.sampleRate * durata);
    const buf = a.createBuffer(1, n, a.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < n; i++) {
      const k = i / n;
      const inv = Math.sin(Math.PI * Math.min(1, k * 1.15)) ** 0.7;           // sale e scende
      const setole = Math.random() < 0.012 ? (Math.random() * 2 - 1) * 2.5 : 0; // crepitio
      d[i] = ((Math.random() * 2 - 1) + setole) * inv;
    }
    const s = a.createBufferSource(); s.buffer = buf;
    const bp = a.createBiquadFilter(); bp.type = "bandpass"; bp.Q.value = 0.9;
    bp.frequency.setValueAtTime(1800, T); bp.frequency.exponentialRampToValueAtTime(5200, T + durata);
    const hp = a.createBiquadFilter(); hp.type = "highpass"; hp.frequency.value = 700;
    const g = a.createGain(); g.gain.value = 0.55 * forza;
    s.connect(bp).connect(hp).connect(g).connect(uscita());
    s.start(T);
  }

  /** Campana/campanello: parziali inarmoniche che si spengono lentamente. */
  function campana(t0, f, vol = 0.2) {
    [[1, 1], [2.76, 0.5], [5.4, 0.25], [8.9, 0.12]].forEach(([k, v]) => tono(f * k, t0, 1.4 / Math.sqrt(k), { vol: vol * v }));
  }

  /** Nota di ottone (tromba): dente di sega filtrato con apertura del filtro. */
  function ottone(f, t0, durata, vol = 0.12) {
    const a = ac(); if (!a) return;
    const T = a.currentTime + t0;
    const o = a.createOscillator(); o.type = "sawtooth"; o.frequency.value = f;
    const lp = a.createBiquadFilter(); lp.type = "lowpass"; lp.Q.value = 2;
    lp.frequency.setValueAtTime(600, T); lp.frequency.linearRampToValueAtTime(3200, T + 0.06); lp.frequency.linearRampToValueAtTime(1800, T + durata);
    const g = a.createGain();
    g.gain.setValueAtTime(0.0001, T); g.gain.linearRampToValueAtTime(vol, T + 0.03);
    g.gain.setValueAtTime(vol, T + durata - 0.05); g.gain.linearRampToValueAtTime(0.0001, T + durata);
    o.connect(lp).connect(g).connect(uscita());
    o.start(T); o.stop(T + durata + 0.05);
  }

  const SINTESI = {
    carta: () => rumore(0, 0.09, { vol: 0.35, freq: 2600, q: 0.8 }),
    distribuzione: () => { for (let i = 0; i < 6; i++) rumore(i * 0.07, 0.06, { vol: 0.25, freq: 3000 }); },
    presa: () => { rumore(0, 0.16, { vol: 0.3, freq: 1500, sweep: 0.5 }); },
    // Scopa: tre colpi di spazzola sul panno e un tintinnio finale
    scopa: () => { spazzolata(0, 0.32, 0.9); spazzolata(0.3, 0.3, 0.8); spazzolata(0.58, 0.5, 1.0); [2093, 2637, 3136].forEach((f, i) => tono(f, 1.05 + i * 0.06, 0.25, { vol: 0.05 })); },
    // carta strisciata sul panno: tre sfregamenti secchi
    striscia: () => { [0, 0.26, 0.52, 0.78].forEach((t, i) => rumore(t, 0.2, { vol: 0.35, freq: i % 2 ? 2600 : 1900, q: 1.4, sweep: i % 2 ? 0.7 : 1.4 })); },
    // ticchettio del timer
    tic: () => { tono(1800, 0, 0.05, { tipo: "square", vol: 0.06 }); },
    // prese buone
    settebello: () => { campana(0, 1568, 0.22); campana(0.14, 2093, 0.2); campana(0.28, 3136, 0.16); },
    rebello: () => { [392, 523.25, 659.25].forEach((f, i) => ottone(f, i * 0.13, i === 2 ? 0.7 : 0.14)); ottone(784, 0.39, 0.7); },
    sette: () => { tono(1318.5, 0, 0.16, { tipo: "triangle", vol: 0.3 }); tono(1975.5, 0.09, 0.35, { tipo: "triangle", vol: 0.26 }); },
    bazzica: () => { [392, 523, 659, 784, 659, 1047].forEach((f, i) => tono(f, i * 0.1, 0.32, { tipo: "square", vol: 0.08 })); tono(131, 0, 0.8, { tipo: "sawtooth", vol: 0.05 }); },
    // Piccola: "zip" fulmineo verso l'alto + due blip rapidissimi
    piccola: () => { tono(400, 0, 0.16, { tipo: "sawtooth", vol: 0.08, slide: 6 }); rumore(0, 0.18, { vol: 0.35, freq: 1500, sweep: 6, q: 3 }); tono(1760, 0.16, 0.05, { vol: 0.15 }); tono(2350, 0.22, 0.06, { vol: 0.15 }); },
    // Grande: caduta (sibilo che scende), poi botto profondo e rimbombo
    grandeCaduta: () => { tono(1400, 0, 0.6, { tipo: "sine", vol: 0.08, slide: 0.2 }); rumore(0, 0.6, { vol: 0.15, freq: 3000, sweep: 0.15, q: 2 }); },
    grande: () => { tono(110, 0, 1.2, { tipo: "sine", vol: 0.6, slide: 0.3 }); tono(55, 0, 1.6, { tipo: "triangle", vol: 0.5, slide: 0.6 }); rumore(0, 0.9, { vol: 0.9, freq: 300, tipo: "lowpass", sweep: 0.3, q: 1 }); rumore(0.05, 1.4, { vol: 0.4, freq: 120, tipo: "lowpass", q: 0.5 }); for (let i = 0; i < 6; i++) rumore(0.15 + i * 0.12, 0.08, { vol: 0.25, freq: 900 + Math.random() * 1500 }); [196, 185, 175, 165].forEach((f, i) => tono(f, 1.0 + i * 0.28, 0.4, { tipo: "triangle", vol: 0.1 })); },
    asso: () => { tono(660, 0, 0.15, { tipo: "triangle", vol: 0.18 }); tono(990, 0.08, 0.25, { tipo: "triangle", vol: 0.18 }); },
    napola: () => { [523, 659, 784, 1047, 1319, 1568].forEach((f, i) => tono(f, i * 0.09, 0.6, { tipo: "triangle", vol: 0.13 })); },
    // Bazziche: folla dello stadio, sempre più scatenata
    bazzica2: () => { ooh(0, 1.6); applausi(1.0, 1.2, 35, 0.08); },
    bazzica3: () => { ooh(0, 1.4); boato(1.0, 2.6, { vol: 0.35, picco: 0.2 }); applausi(1.1, 2.2, 60, 0.1); },
    bazzica5: () => { ooh(0, 1.2, 0.17, 22); boato(0.9, 4.2, { vol: 0.35, picco: 0.15 }); trombetta(1.0, 0.7); applausi(1.0, 3, 70, 0.1); coroBazzica(1.8, 1, 18); battimani(1.8, 1, 0.45); },
    tamburo: () => { for (let i = 0; i < 26; i++) rumore(i * 0.045, 0.05, { vol: 0.15 + i * 0.012, freq: 180, tipo: "lowpass", q: 2 }); boato(0, 1.4, { vol: 0.25, picco: 0.95, freq: 600 }); },
    bazzica7: () => {
      // boato esplosivo, trombette, poi la curva intera canta "Baz-zi-caaa!" due volte con i battimani
      boato(0, 6.5, { vol: 0.45, picco: 0.08, freq: 1100 });
      boato(0, 6.5, { vol: 0.25, picco: 0.1, freq: 450 });
      tono(45, 0, 1.6, { tipo: "sine", vol: 0.5, slide: 0.6 });
      trombetta(0.15, 0.6, 0.16); trombetta(0.85, 0.9, 0.16);
      applausi(0.2, 6, 100, 0.1);
      coroBazzica(1.6, 2, 26);
      battimani(1.6, 2, 0.6);
      trombetta(5.0, 1.2, 0.14);
    },
    tuono: () => { rumore(0, 1.4, { vol: 0.9, freq: 120, tipo: "lowpass", sweep: 0.5, q: 0.7 }); rumore(0, 0.15, { vol: 0.7, freq: 2500, tipo: "highpass" }); },
    moneta: () => { tono(1568, 0, 0.08, { tipo: "square", vol: 0.07 }); tono(2093, 0.07, 0.25, { tipo: "square", vol: 0.07 }); },
    napoleone: () => { [392, 392, 392, 523, 659, 784, 1047].forEach((f, i) => tono(f, i * 0.16, i === 6 ? 1.2 : 0.18, { tipo: "square", vol: 0.09 })); [523, 659, 784].forEach((f) => tono(f, 1.1, 1.5, { tipo: "triangle", vol: 0.12 })); },
    festa: () => { for (let i = 0; i < 8; i++) { rumore(i * 0.3, 0.25, { vol: 0.4, freq: 900, sweep: 4 }); rumore(i * 0.3 + 0.25, 0.4, { vol: 0.5, freq: 2500, tipo: "highpass" }); } },
    triste: () => { [392, 370, 349, 330].forEach((f, i) => tono(f, i * 0.35, i === 3 ? 0.9 : 0.35, { tipo: "sawtooth", vol: 0.07, slide: i === 3 ? 0.94 : 1 })); },
    palo: () => { tono(300, 0, 0.35, { tipo: "sine", vol: 0.3, slide: 2.2 }); tono(660, 0.3, 0.25, { tipo: "sine", vol: 0.2, slide: 0.6 }); },
    fuoco: () => { rumore(0, 0.9, { vol: 0.5, freq: 400, tipo: "lowpass", sweep: 5, q: 2 }); for (let i = 0; i < 10; i++) rumore(0.05 + Math.random() * 0.7, 0.03, { vol: 0.3, freq: 5000 }); tono(90, 0, 0.5, { tipo: "sawtooth", vol: 0.08, slide: 0.5 }); },
    passa: () => rumore(0, 0.18, { vol: 0.25, freq: 1800, sweep: 2 }),
    turno: () => tono(740, 0, 0.12, { tipo: "sine", vol: 0.12 }),
    fineMano: () => { [392, 494, 587].forEach((f) => tono(f, 0, 0.9, { tipo: "triangle", vol: 0.1 })); },
    vittoria: () => { [523, 523, 523, 659, 587, 659, 784].forEach((f, i) => tono(f, i * 0.14, 0.3, { tipo: "square", vol: 0.08 })); },
  };

  function suona(nome) {
    if (muto) return;
    if (FILE[nome]) {
      try {
        const base = PRECARICATI[nome];
        const a = base ? base.cloneNode(true) : new Audio(FILE[nome]);
        a.volume = 0.9;
        const p = a.play();
        if (p && p.catch) p.catch(() => {});
        return;
      } catch (e) { /* fallback alla sintesi */ }
    }
    try { SINTESI[nome] && SINTESI[nome](); } catch (e) { /* audio non disponibile */ }
  }

  // =================================================================== contesto e carte
  let C = { io: 0, miei: [0], nomi: POSTI.slice(), squadre: ["Squadra 1", "Squadra 2"], mazzo: "originale" };
  function normalizzaCtx(c) {
    const b = Object.assign({}, C);
    if (c && typeof c === "object") for (const k of Object.keys(c)) if (c[k] !== undefined && c[k] !== null) b[k] = c[k];
    b.io = Number(b.io) || 0;
    b.miei = Array.isArray(b.miei) ? b.miei.map(Number) : [b.io];
    return b;
  }
  const nome = (s) => (C.nomi && C.nomi[s]) || POSTI[s] || "?";
  const nomeSq = (t) => (C.squadre && C.squadre[t]) || `Squadra ${t + 1}`;
  const mio = (s) => s === C.io || C.miei.includes(s);

  const piac = () => C.mazzo === "piacentine";
  const RAPPORTO = () => (piac() ? 0.565 : 0.6);  // larghezza / altezza della carta
  const urlCarta = (c) => (piac() ? `piacentine/${c}.webp` : `carte/${c}.svg`);
  const urlDorso = () => (piac() ? "piacentine/dorso.webp" : "carte/dorso.svg");

  function carta(c, opts = {}) {
    if (!c || c === "?") return dorso(opts);
    const e = el("div", { class: "fx-carta" + (piac() ? " fx-piac" : "") + (opts.class ? " " + opts.class : ""), title: nomeCarta(c) });
    e.style.backgroundImage = `url("${urlCarta(c)}")`;
    return e;
  }
  function dorso(opts = {}) {
    const e = el("div", { class: "fx-carta fx-dorso" + (piac() ? " fx-piac" : "") + (opts.class ? " " + opts.class : "") });
    e.style.backgroundImage = `url("${urlDorso()}")`;
    return e;
  }
  const precaricati = new Set();
  function precarica() {
    if (precaricati.has(C.mazzo)) return;
    precaricati.add(C.mazzo);
    const urls = [urlDorso()];
    for (const s of SEMI) for (let v = 1; v <= 10; v++) urls.push(urlCarta(`${s}_${String(v).padStart(2, "0")}`));
    for (const u of urls) { try { const i = new Image(); i.src = u; } catch (e) { /* ignora */ } }
  }

  // =================================================================== livello degli effetti
  function layer() {
    let l = document.getElementById("fx");
    if (!l) {
      l = el("div", { id: "fx", "aria-hidden": "true" });
      document.body.append(l);
    }
    return l;
  }
  const VELOCITA = 1; // moltiplicatore delle durate
  const centro = (r) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  const bersaglioScossa = () => document.querySelector(".partita") || document.body;

  let toastTimer = null;
  function toast(msg, ms = 2800) {
    let t = layer().querySelector(".fx-toast");
    if (!t) { t = el("div", { class: "fx-toast fx-nascosto", role: "status" }); layer().append(t); }
    t.textContent = msg;
    t.classList.remove("fx-nascosto");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => t.classList.add("fx-nascosto"), ms);
  }

  // =================================================================== primitive (da anim.js)
  /** Fa volare un elemento carta da ``da`` (DOMRect) ad ``a`` (DOMRect). */
  async function vola(cartaEl, da, a, { durata = 450, fiamme = false, ritardo = 0, svanisci = false, ruota = 0 } = {}) {
    if (!da || !a) return;
    if (!da.width) da = a;
    durata *= VELOCITA;
    const w = a.width || da.width;
    const box = el("div", { class: "fx-volante" + (fiamme ? " fx-in-fiamme" : "") });
    box.style.width = da.width + "px";
    box.style.height = da.height + "px";
    box.style.left = da.left + "px";
    box.style.top = da.top + "px";
    box.style.transitionDuration = `${durata}ms, 300ms`;
    if (fiamme && !ridotto()) box.append(el("div", { class: "fx-alone-fuoco" }));
    box.append(cartaEl);
    layer().append(box);
    if (ritardo) await sleep(ritardo);
    await dopoFrame();
    const dx = a.left - da.left + (a.width - da.width) / 2;
    const dy = a.top - da.top + (a.height - da.height) / 2;
    const sc = w / da.width;
    box.style.transform = `translate(${dx}px, ${dy}px) scale(${sc}) rotate(${ridotto() ? 0 : ruota}deg)`;
    if (svanisci) box.style.opacity = "0";
    let timer = null;
    if (fiamme && !ridotto()) {
      timer = setInterval(() => {
        const r = box.getBoundingClientRect();
        for (let i = 0; i < 3; i++) fiammella(r.left + Math.random() * r.width, r.top + Math.random() * r.height);
      }, 30);
    }
    await sleep(durata + 30);
    if (timer) clearInterval(timer);
    box.remove();
  }

  function fiammella(x, y) {
    const f = el("div", { class: "fx-fiamma" });
    f.style.left = x + "px";
    f.style.top = y + "px";
    f.style.setProperty("--dx", (Math.random() * 30 - 15) + "px");
    const s = 0.7 + Math.random() * 1.2;
    f.style.width = f.style.height = 18 * s + "px";
    layer().append(f);
    setTimeout(() => f.remove(), 750);
  }

  function banner(testo, classe = "", sotto = "", durata = 1600) {
    const b = el("div", { class: "fx-banner " + classe }, testo, sotto ? el("small", {}, sotto) : null);
    layer().append(b);
    setTimeout(() => b.remove(), durata + 100);
    return sleep(Math.min(durata, 1100) * VELOCITA);
  }

  function scintille(x, y, n = 24, colore = null) {
    if (ridotto()) n = Math.min(n, 8);
    for (let i = 0; i < n; i++) {
      const s = el("div", { class: "fx-scintilla" });
      const ang = Math.random() * Math.PI * 2, d = 60 + Math.random() * 140;
      s.style.left = x + "px";
      s.style.top = y + "px";
      s.style.setProperty("--dx", Math.cos(ang) * d + "px");
      s.style.setProperty("--dy", Math.sin(ang) * d + "px");
      if (colore) { s.style.background = colore; s.style.boxShadow = `0 0 8px ${colore}`; }
      layer().append(s);
      setTimeout(() => s.remove(), 1050);
    }
  }

  function fumetto(rect, testo) {
    const f = el("div", { class: "fx-fumetto" }, testo);
    const c = centro(rect);
    f.style.left = Math.min(window.innerWidth - 120, Math.max(120, c.x)) + "px";
    f.style.top = Math.max(70, rect.top - 6) + "px";
    layer().append(f);
    setTimeout(() => f.remove(), 2500);
  }

  function scuoti(elem) {
    if (!elem || ridotto()) return;
    elem.classList.remove("fx-scuoti");
    void elem.offsetWidth;
    elem.classList.add("fx-scuoti");
    setTimeout(() => elem.classList.remove("fx-scuoti"), 500);
  }

  function lampeggia(elems, classe = "fx-evidenzia", ms = 500) {
    elems.forEach((e) => e && e.classList.add(classe));
    return sleep(ms * VELOCITA);
  }

  // ----------------------------------------------------------------- Piccola: velocissima
  function lineeVelocita(x, y) {
    if (ridotto()) return;
    const d = el("div", { class: "fx-linee-velocita" });
    d.style.setProperty("--x", x + "px");
    d.style.setProperty("--y", y + "px");
    layer().append(d);
    setTimeout(() => d.remove(), 600);
  }

  function saetta(da, a) {
    if (ridotto()) return;
    const dx = a.x - da.x, dy = a.y - da.y;
    const s = el("div", { class: "fx-saetta" });
    s.style.left = da.x + "px";
    s.style.top = da.y + "px";
    s.style.width = Math.hypot(dx, dy) + "px";
    s.style.transform = `rotate(${Math.atan2(dy, dx)}rad)`;
    layer().append(s);
    setTimeout(() => s.remove(), 400);
  }

  function sbuffi(x, y, n = 10) {
    for (let i = 0; i < n; i++) {
      const p = el("div", { class: "fx-sbuffo" });
      const ang = Math.random() * Math.PI * 2, d = 40 + Math.random() * 60;
      p.style.left = x + "px";
      p.style.top = y + "px";
      p.style.setProperty("--dx", Math.cos(ang) * d + "px");
      p.style.setProperty("--dy", Math.sin(ang) * d + "px");
      layer().append(p);
      setTimeout(() => p.remove(), 550);
    }
  }

  function bannerZip(testo, sotto = "") {
    const b = el("div", { class: "fx-banner-zip" }, testo, sotto ? el("small", {}, sotto) : null);
    layer().append(b);
    setTimeout(() => b.remove(), 1100);
    return sleep(900 * VELOCITA);
  }

  // ----------------------------------------------------------------- Grande: pesantissima
  function vignetta() {
    if (ridotto()) return;
    const v = el("div", { class: "fx-vignetta" });
    layer().append(v);
    setTimeout(() => v.remove(), 2700);
  }

  /** Una carta enorme precipita dall'alto e si schianta sul punto ``dst`` (DOMRect). */
  async function macigno(cartaEl, dst, durata = 620) {
    durata *= VELOCITA;
    const c = centro(dst);
    const ombra = el("div", { class: "fx-ombra-impatto" });
    ombra.style.left = c.x + "px";
    ombra.style.top = c.y + dst.height * 0.45 + "px";
    ombra.style.width = dst.width * 2.2 + "px";
    ombra.style.height = dst.width * 0.8 + "px";
    ombra.style.animationDuration = durata + "ms";
    layer().append(ombra);
    const box = el("div", { class: "fx-macigno" });
    box.style.left = dst.left + "px";
    box.style.top = dst.top + "px";
    box.style.width = dst.width + "px";
    box.style.height = dst.height + "px";
    box.style.transform = ridotto() ? "translateY(-40px)" : `translateY(${-window.innerHeight * 0.75}px) scale(3.2) rotate(-14deg)`;
    box.style.transition = `transform ${durata}ms cubic-bezier(.6,0,1,.6)`;
    box.append(cartaEl);
    layer().append(box);
    await dopoFrame();
    box.style.transform = "translateY(0) scale(1) rotate(0)";
    await sleep(durata);
    ombra.remove();
    return { box, c };
  }

  function crepe(x, y, r = 170) {
    if (ridotto()) return;
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", "fx-crepe");
    svg.setAttribute("width", r * 2);
    svg.setAttribute("height", r * 2);
    svg.style.left = x + "px";
    svg.style.top = y + "px";
    const n = 9;
    for (let i = 0; i < n; i++) {
      let ang = (i / n) * Math.PI * 2 + Math.random() * 0.4;
      let px = r, py = r, d = "M" + px + " " + py;
      const passi = 4 + Math.floor(Math.random() * 3);
      const lung = r * (0.55 + Math.random() * 0.45);
      for (let k = 0; k < passi; k++) {
        ang += (Math.random() - 0.5) * 0.9;
        px += Math.cos(ang) * (lung / passi);
        py += Math.sin(ang) * (lung / passi);
        d += ` L${px.toFixed(1)} ${py.toFixed(1)}`;
      }
      const p = document.createElementNS(ns, "path");
      p.setAttribute("d", d);
      p.style.animationDelay = Math.random() * 120 + "ms";
      p.style.strokeWidth = 3 + Math.random() * 4;
      svg.append(p);
    }
    layer().append(svg);
    setTimeout(() => svg.remove(), 2900);
  }

  function polvere(x, y, n = 18) {
    if (ridotto()) return;
    for (let i = 0; i < n; i++) {
      const p = el("div", { class: "fx-polvere" });
      const lato = i % 2 ? 1 : -1;
      p.style.left = x + (Math.random() * 40 - 20) + "px";
      p.style.top = y + 20 + "px";
      p.style.setProperty("--dx", lato * (60 + Math.random() * 200) + "px");
      p.style.setProperty("--dy", -(Math.random() * 90) + "px");
      p.style.setProperty("--s", 2 + Math.random() * 3);
      p.style.animationDelay = Math.random() * 120 + "ms";
      layer().append(p);
      setTimeout(() => p.remove(), 1500);
    }
  }

  function terremoto(elem) {
    if (!elem || ridotto()) return;
    elem.classList.remove("fx-terremoto");
    void elem.offsetWidth;
    elem.classList.add("fx-terremoto");
    setTimeout(() => elem.classList.remove("fx-terremoto"), 750);
  }

  function bannerPesante(testo, sotto = "") {
    const b = el("div", { class: "fx-banner-pesante" }, testo, sotto ? el("small", {}, sotto) : null);
    layer().append(b);
    setTimeout(() => b.remove(), 2350);
    return b;
  }

  function lacrima(rect, simbolo = "😱") {
    const l = el("div", { class: "fx-lacrima" }, simbolo);
    l.style.left = rect.left + rect.width / 2 + "px";
    l.style.top = rect.top - 10 + "px";
    layer().append(l);
    setTimeout(() => l.remove(), 2300);
  }

  // ----------------------------------------------------------------- festa e devastazione
  const COLORI_FESTA = ["#ff4fa3", "#ffd23f", "#5ad1ff", "#7be37b", "#ff7a00", "#b28dff", "#ffffff"];

  function coriandoli(n = 80, durata = 2600) {
    if (ridotto()) return;
    for (let i = 0; i < n; i++) {
      const c = el("div", { class: "fx-coriandolo" });
      c.style.left = Math.random() * 100 + "vw";
      c.style.background = COLORI_FESTA[i % COLORI_FESTA.length];
      c.style.setProperty("--dx", (Math.random() * 200 - 100) + "px");
      c.style.setProperty("--r", (Math.random() * 1440 - 720) + "deg");
      c.style.setProperty("--t", (durata * (0.6 + Math.random() * 0.6)) + "ms");
      c.style.animationDelay = Math.random() * 600 + "ms";
      if (i % 3 === 0) c.style.borderRadius = "50%";
      layer().append(c);
      setTimeout(() => c.remove(), durata * 1.3 + 700);
    }
  }

  function flash(colore = "#fff", t = 350) {
    if (ridotto()) return;
    const f = el("div", { class: "fx-flash" });
    f.style.setProperty("--col", colore);
    f.style.setProperty("--t", t + "ms");
    layer().append(f);
    setTimeout(() => f.remove(), t + 50);
  }

  function raggi(colore = "rgba(255,79,163,.35)", t = 2600) {
    if (ridotto()) return;
    const r = el("div", { class: "fx-raggi" });
    r.style.setProperty("--col", colore);
    r.style.setProperty("--t", t + "ms");
    layer().append(r);
    setTimeout(() => r.remove(), t + 50);
  }

  function onda(x, y, colore = "#fff", scala = 30) {
    if (ridotto()) return;
    const o = el("div", { class: "fx-onda" });
    o.style.left = x + "px";
    o.style.top = y + "px";
    o.style.setProperty("--col", colore);
    o.style.setProperty("--s", scala);
    layer().append(o);
    setTimeout(() => o.remove(), 950);
  }

  function buio(t = 5000) {
    const b = el("div", { class: "fx-buio" });
    b.style.setProperty("--t", t + "ms");
    layer().append(b);
    setTimeout(() => b.remove(), t + 50);
  }

  function fulmine() {
    if (ridotto()) return;
    const ns = "http://www.w3.org/2000/svg";
    const W = window.innerWidth, H = window.innerHeight;
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", "fx-fulmine");
    svg.setAttribute("width", W);
    svg.setAttribute("height", H);
    svg.style.left = "0px";
    svg.style.top = "0px";
    let x = Math.random() * W, y = 0, d = `M${x} ${y}`;
    while (y < H * (0.6 + Math.random() * 0.4)) {
      x += Math.random() * 120 - 60;
      y += 30 + Math.random() * 50;
      d += ` L${x.toFixed(0)} ${y.toFixed(0)}`;
    }
    const p = document.createElementNS(ns, "path");
    p.setAttribute("d", d);
    svg.append(p);
    layer().append(svg);
    setTimeout(() => svg.remove(), 520);
  }

  function carteGiganti(elems, t = 2600, vortice = false) {
    const box = el("div", { class: "fx-carte-giganti" + (vortice && !ridotto() ? " fx-vortice" : "") });
    box.style.setProperty("--t", t + "ms");
    elems.forEach((e) => { e.style.setProperty("--t", t + "ms"); box.append(e); });
    layer().append(box);
    setTimeout(() => box.remove(), t + 400);
  }

  function pioggiaCarte(fabbrica, n = 40, durata = 2200) {
    if (ridotto()) return;
    for (let i = 0; i < n; i++) {
      const w = el("div", { class: "fx-pioggia-carta" + (piac() ? " fx-piac" : "") });
      w.append(fabbrica(i));
      w.style.left = Math.random() * 100 + "vw";
      w.style.setProperty("--dx", (Math.random() * 300 - 150) + "px");
      w.style.setProperty("--r", (Math.random() * 1080 - 540) + "deg");
      w.style.setProperty("--t", durata * (0.7 + Math.random() * 0.5) + "ms");
      w.style.animationDelay = Math.random() * 900 + "ms";
      layer().append(w);
      setTimeout(() => w.remove(), durata * 1.3 + 1000);
    }
  }

  function fuocoArtificio(x, y, n = 36) {
    const col = COLORI_FESTA[Math.floor(Math.random() * COLORI_FESTA.length)];
    for (let i = 0; i < n; i++) {
      const s = el("div", { class: "fx-fuoco-artificio" });
      const ang = (i / n) * Math.PI * 2, d = 80 + Math.random() * 90;
      s.style.left = x + "px";
      s.style.top = y + "px";
      s.style.background = col;
      s.style.boxShadow = `0 0 8px ${col}, 0 0 16px ${col}`;
      s.style.setProperty("--dx", Math.cos(ang) * d + "px");
      s.style.setProperty("--dy", Math.sin(ang) * d + "px");
      layer().append(s);
      setTimeout(() => s.remove(), 1250);
    }
  }

  function fuochi(n = 6, intervallo = 280) {
    if (ridotto()) return;
    for (let i = 0; i < n; i++) {
      setTimeout(() => fuocoArtificio(window.innerWidth * (0.15 + Math.random() * 0.7), window.innerHeight * (0.15 + Math.random() * 0.4)), i * intervallo);
    }
  }

  /** Parata delle carte di denari: arrivano una alla volta e si dispongono ad arco. */
  async function parataDenari(elemsCarte, passo = 260, onCarta = null) {
    const box = el("div", { class: "fx-parata-denari" });
    layer().append(box);
    const n = elemsCarte.length;
    const W = box.getBoundingClientRect().width, H = box.getBoundingClientRect().height;
    elemsCarte.forEach((c) => box.append(c));
    await sleep(50);
    for (let i = 0; i < n; i++) {
      const c = elemsCarte[i];
      const t = n === 1 ? 0.5 : i / (n - 1);
      const cw = c.getBoundingClientRect().width || 80;
      const x = t * (W - cw);
      const y = H * 0.55 - Math.sin(t * Math.PI) * H * 0.45;
      const rot = (t - 0.5) * 50;
      c.classList.add("fx-in");
      c.style.transform = `translate(${x}px, ${y}px) rotate(${rot}deg) scale(1)`;
      if (onCarta) onCarta(i);
      await sleep(passo * VELOCITA);
    }
    return box;
  }

  // ----------------------------------------------------------------- Scopa: la spazzola
  /** Una spazzola attraversa il tavolo da sinistra a destra lasciando polvere e scintille. */
  async function spazzola(rect, durata = 900) {
    durata *= VELOCITA;
    const s = el("div", { class: "fx-spazzola" }, "🧹");
    const y = rect.top + rect.height / 2;
    s.style.top = y + "px";
    s.style.left = rect.left - 40 + "px";
    s.style.setProperty("--dist", rect.width + 80 + "px");
    s.style.animationDuration = durata + "ms";
    layer().append(s);
    const t0 = performance.now();
    const timer = setInterval(() => {
      const k = Math.min(1, (performance.now() - t0) / durata);
      const x = rect.left - 40 + k * (rect.width + 80);
      for (let i = 0; i < 2; i++) {
        const p = el("div", { class: "fx-polverina" });
        p.style.left = x - 10 + Math.random() * 20 + "px";
        p.style.top = y + 10 + Math.random() * 30 + "px";
        p.style.setProperty("--dx", 30 + Math.random() * 40 + "px");
        p.style.setProperty("--dy", -(10 + Math.random() * 40) + "px");
        layer().append(p);
        setTimeout(() => p.remove(), 700);
      }
    }, 35);
    await sleep(durata);
    clearInterval(timer);
    s.remove();
  }

  /** Banner con le lettere che cadono una alla volta e rimbalzano. */
  function bannerLettere(testo, sotto = "", classe = "") {
    const b = el("div", { class: "fx-banner-lettere " + classe });
    const parola = el("div", { class: "fx-parola" });
    [...testo].forEach((ch, i) => {
      const l = el("span", { class: "fx-lettera" }, ch);
      l.style.animationDelay = i * 70 + "ms";
      l.style.setProperty("--rot", (Math.random() * 24 - 12) + "deg");
      parola.append(l);
    });
    b.append(parola);
    if (sotto) b.append(el("small", {}, sotto));
    layer().append(b);
    setTimeout(() => b.remove(), 1900);
    return sleep(1300 * VELOCITA);
  }

  // ----------------------------------------------------------------- prese buone
  /** La carta presa sale al centro, brilla con la sua scritta e poi vola nel mazzo delle prese. */
  async function cartaDiGloria(cartaEl, testo, stile, dest, durata = 1100) {
    durata *= VELOCITA;
    const box = el("div", { class: "fx-gloria fx-" + stile });
    const alone = el("div", { class: "fx-gloria-alone" });
    box.append(alone, cartaEl);
    if (stile === "re") box.append(el("div", { class: "fx-gloria-corona" }, "👑"));
    box.append(el("div", { class: "fx-gloria-testo" }, testo));
    box.style.setProperty("--durata", durata + "ms");
    layer().append(box);
    const r = box.getBoundingClientRect();
    scintille(r.left + r.width / 2, r.top + r.height * 0.4, stile === "primiera" ? 12 : 26,
      stile === "primiera" ? "#ff8a5c" : "#ffd23f");
    await sleep(durata * 0.75);
    // vola nel mazzo delle prese
    const c = centro(dest), cr = centro(cartaEl.getBoundingClientRect());
    box.style.animation = "none";                       // l'animazione d'ingresso non deve bloccare il transform
    box.style.transform = "translate(-50%, -50%)";
    await dopoFrame();
    box.style.transition = `transform ${durata * 0.25}ms ease-in, opacity ${durata * 0.25}ms ease-in`;
    box.style.transform = `translate(calc(-50% + ${c.x - cr.x}px), calc(-50% + ${c.y - cr.y}px)) scale(.25)`;
    box.style.opacity = "0.2";
    await sleep(durata * 0.25);
    box.remove();
  }

  // ----------------------------------------------------------------- "Striscia"
  /** Un dorso di carta strisciato avanti e indietro sul panno, per mettere fretta. */
  function cartaStrisciata(cartaEl, x, y) {
    const w = el("div", { class: "fx-carta-strisciata" + (piac() ? " fx-piac" : "") });
    w.append(cartaEl);
    w.style.left = x + "px";
    w.style.top = y + "px";
    layer().append(w);
    setTimeout(() => w.remove(), 1250);
  }

  // =================================================================== geometria del tavolo
  const nelFx = (e) => !!(e.closest && e.closest("#fx"));
  const tutti = (sel) => [...document.querySelectorAll(sel)].filter((e) => !nelFx(e));
  const zonaDi = (e) => (e.closest(".tavola") ? "tavola" : e.closest(".carte-mano") ? "mano" : "altro");
  const visibile = (r) => !!r && r.width > 0 && r.height > 0;
  const rettangolo = (l, t, w, h) => new DOMRect(l, t, w, h);
  const centrata = (r, w, h) => rettangolo(r.left + r.width / 2 - w / 2, r.top + r.height / 2 - h / 2, w, h);

  let foto = null;        // ultima istantanea non ancora usata
  let F = null;           // istantanea del lotto in corso (per i ripieghi)
  let daPreparare = [];   // istantanee di lotti già renderizzati ma non ancora preparati

  /** Il box di un posto: per chi guarda, la sua mano (.carte-mano); per gli altri il loro riquadro. */
  function elPosto(seat) {
    const all = tutti(`[data-seat="${seat}"]`);
    return all.find((e) => e.matches(".carte-mano")) || all[0] || null;
  }
  /** Dove mostrare le reazioni di un giocatore (e a chi applicare le classi di umore). */
  function avatarDi(seat) {
    const box = tutti(`[data-seat="${seat}"]`).find((e) => !e.matches(".carte-mano"));
    if (box) {
      const av = box.querySelector(".avatar, .avatar-piccolo");
      const r = (av || box).getBoundingClientRect();
      if (visibile(r)) return { rect: r, el: av || box };
    }
    return { rect: rectPosto(seat), el: null };
  }

  function rectPosto(seat) {
    const e = elPosto(seat);
    if (e) { const r = e.getBoundingClientRect(); if (visibile(r)) return r; }
    if (F) {
      const r = seat === C.io ? (F.mani[seat] || F.box[seat]) : (F.box[seat] || F.mani[seat]);
      if (r) return r;
    }
    // ripiego: lati dello schermo, relativi a chi guarda
    const W = window.innerWidth, H = window.innerHeight, p = ((seat - C.io) % 4 + 4) % 4;
    const w = 120, h = 80;
    const pos = [[W / 2, H - h / 2 - 10], [w / 2 + 10, H / 2], [W / 2, h / 2 + 10], [W - w / 2 - 10, H / 2]][p];
    return rettangolo(pos[0] - w / 2, pos[1] - h / 2, w, h);
  }

  function rectTavola() {
    const t = tutti(".tavola")[0];
    if (t) { const r = t.getBoundingClientRect(); if (visibile(r)) return r; }
    if (F && F.tavola) return F.tavola;
    const W = window.innerWidth, H = window.innerHeight;
    return rettangolo(W * 0.25, H * 0.35, W * 0.5, H * 0.25);
  }

  function dimCarta() {
    let w = 0;
    for (const e of tutti(".tavola [data-carta]")) { const r = e.getBoundingClientRect(); if (visibile(r)) { w = r.width; break; } }
    if (!w && F) for (const v of F.carte.values()) if (v.zona === "tavola") { w = v.r.width; break; }
    if (!w) w = Math.max(44, Math.min(80, window.innerWidth * 0.09));
    return { w, h: w / RAPPORTO() };
  }

  function rectCartaTavola() {
    const t = rectTavola();
    const { w, h } = dimCarta();
    return rettangolo(t.left + t.width / 2 - w / 2 + (Math.random() * 40 - 20), t.top + t.height / 2 - h / 2, w, h);
  }

  function rectPrese(team) {
    let r = null;
    const e = tutti(`[data-prese="${team}"]`)[0];
    if (e) { const x = e.getBoundingClientRect(); if (visibile(x)) r = x; }
    if (!r && F && F.prese[team]) r = F.prese[team];
    if (!r) {
      const W = window.innerWidth, H = window.innerHeight;
      r = team === squadra(C.io) ? rettangolo(10, H - 110, 60, 100) : rettangolo(W - 70, 10, 60, 100);
    }
    const w = Math.max(24, dimCarta().w * 0.6);
    return centrata(r, w, w / RAPPORTO());
  }

  /** Da dove parte una carta giocata: la sua posizione memorizzata, oppure il posto del giocatore. */
  function rectSorgente(code, seat) {
    const vecchio = F && F.carte.get(code);
    if (vecchio && vecchio.zona !== "tavola") return vecchio.r;
    const { w, h } = dimCarta();
    return centrata(rectPosto(seat), w * 0.8, h * 0.8);
  }

  // =================================================================== FLIP: nascoste, spostate, fantasmi
  const S = { fantasmi: new Map(), nascosti: new Set(), invertiti: new Set() };

  function nascondi(e) { e.style.visibility = "hidden"; S.nascosti.add(e); }
  function rivelaEl(e) { e.style.visibility = ""; S.nascosti.delete(e); }
  function rivela(code) { for (const e of [...S.nascosti]) if (e.dataset.carta === code) rivelaEl(e); }
  function rivelaTutti() { for (const e of [...S.nascosti]) rivelaEl(e); }
  function nascostoInTavola(code) {
    return tutti(`.tavola [data-carta="${code}"]`)[0] || null;
  }

  function inverti(e, vecchio, ora) {
    const dx = vecchio.left - ora.left, dy = vecchio.top - ora.top;
    if (Math.abs(dx) + Math.abs(dy) < 1) return;
    e.style.transition = "none";
    e.style.transform = `translate(${dx}px, ${dy}px)`;
    S.invertiti.add(e);
  }
  /** Fa scivolare al loro nuovo posto le carte riportate indietro da inverti(). */
  function assesta(zona) {
    for (const e of [...S.invertiti]) {
      if (zona && zonaDi(e) !== zona) continue;
      S.invertiti.delete(e);
      if (!e.isConnected) continue;
      e.style.transition = "transform .3s ease";
      void e.offsetWidth;
      e.style.transform = "";
      setTimeout(() => { if (!e.style.transform) e.style.transition = ""; }, 350);
    }
  }

  function posaFantasma(code, r) {
    togliFantasma(code);
    const g = el("div", { class: "fx-fantasma" });
    g.style.left = r.left + "px";
    g.style.top = r.top + "px";
    g.style.width = r.width + "px";
    g.style.height = r.height + "px";
    g.dataset.fxCarta = code;
    g.append(carta(code));
    layer().append(g);
    S.fantasmi.set(code, g);
    return g;
  }
  function togliFantasma(code) {
    const g = S.fantasmi.get(code);
    if (g) { g.remove(); S.fantasmi.delete(code); }
  }
  /** Il fantasma di una carta della tavola (creato se serve dalla posizione memorizzata). */
  function assicuraFantasma(code) {
    if (S.fantasmi.has(code)) {
      const g = S.fantasmi.get(code);
      g.style.visibility = "";
      return g;
    }
    const vero = nascostoInTavola(code);
    if (vero) {
      const r = vero.getBoundingClientRect();
      if (visibile(r)) { nascondi(vero); return posaFantasma(code, r); }
    }
    const vecchio = F && F.carte.get(code);
    if (vecchio && vecchio.zona === "tavola" && !vecchio.nascosta) return posaFantasma(code, vecchio.r);
    return posaFantasma(code, rectCartaTavola());
  }

  function istantanea() {
    try {
      const carte = new Map();
      const prec = daPreparare[daPreparare.length - 1];
      for (const e of tutti("[data-carta]")) {
        const c = e.dataset.carta;
        if (!c || carte.has(c)) continue;
        const r = e.getBoundingClientRect();
        if (!visibile(r)) continue;
        const zona = zonaDi(e);
        // carta ancora "in arrivo" (nascosta da noi, o nuova in un render non ancora preparato)
        const v = prec && prec.carte.get(c);
        const nascosta = e.style.visibility === "hidden" || (!!prec && (!v || v.zona !== zona));
        carte.set(c, { r, zona, nascosta });
      }
      const box = {}, mani = {};
      for (const e of tutti("[data-seat]")) {
        const s = Number(e.dataset.seat);
        const r = e.getBoundingClientRect();
        if (!visibile(r)) continue;
        if (e.matches(".carte-mano")) { if (!mani[s]) mani[s] = r; } else if (!box[s]) box[s] = r;
      }
      const t = tutti(".tavola")[0];
      const prese = {};
      for (const e of tutti("[data-prese]")) prese[e.dataset.prese] = e.getBoundingClientRect();
      foto = { carte, box, mani, tavola: t ? t.getBoundingClientRect() : null, prese, t: performance.now() };
    } catch (err) { console.warn("[BazzicaFx] istantanea:", err); }
  }

  /** Prepara il DOM appena aggiornato perché sembri ancora quello di prima (FLIP). */
  function prepara(f) {
    if (!f) return;
    F = f;
    const primaPartita = f.carte.size > 0 || Object.keys(f.box).length > 0 || Object.keys(f.mani).length > 0;
    const misure = tutti("[data-carta]").map((e) => ({ e, c: e.dataset.carta, r: e.getBoundingClientRect(), z: zonaDi(e) }));
    const inTavola = new Set(misure.filter((m) => m.z === "tavola").map((m) => m.c));
    for (const m of misure) {
      if (S.nascosti.has(m.e) || S.invertiti.has(m.e)) continue;
      const vecchio = f.carte.get(m.c);
      if (!vecchio) { if (primaPartita) nascondi(m.e); continue; }       // carta nuova: arriva con il suo evento
      if (vecchio.zona !== m.z) { nascondi(m.e); continue; }              // es. dalla mano alla tavola
      if (m.z === "tavola" || m.z === "mano") inverti(m.e, vecchio.r, m.r);
    }
    // carte sparite dalla tavola (prese): restano visibili come fantasmi finché non volano via
    for (const [c, v] of f.carte) {
      if (v.zona === "tavola" && !inTavola.has(c) && !S.fantasmi.has(c)) {
        const g = posaFantasma(c, v.r);
        if (v.nascosta) g.style.visibility = "hidden";   // arriverà con un evento di un lotto precedente
      }
    }
  }

  function pulisci() {
    rivelaTutti();
    assesta();
    for (const g of S.fantasmi.values()) { g.classList.add("fx-svanisci"); setTimeout(() => g.remove(), 320); }
    S.fantasmi.clear();
    F = null;
  }

  // =================================================================== il regista (da game.js)
  // Reazioni automatiche di chi subisce punti: più punti, più disperazione.
  const REAZ = {
    1: { emo: ["😕", "😒", "🙄", "😬"], frasi: ["Uffa", "Mah...", "Vabbè", "Pazienza"] },
    2: { emo: ["😠", "😢", "😤", "😣"], frasi: ["Ma dai!", "Che sfortuna...", "Ancora?!", "Grrr"] },
    3: { emo: ["😡", "😭", "😩", "🤬"], frasi: ["Non è possibile!", "Ma come?!", "Basta!!", "Ingiocabile!"] },
    4: { emo: ["💀", "😱", "🤯", "😵"], frasi: ["È FINITA", "Mi ritiro...", "NOOOOO!", "Chiamo l'avvocato"] },
  };
  const GIOIA = ["😎", "🥳", "😁", "🤩", "😏"];
  const privati = {};   // carta passata in segreto (scambio_carta), per animare lo scambio

  function umore(g, cls, ms) {
    if (!g) return;
    const k = "fx-" + cls;
    g.classList.add(k, "fx-mosso");
    setTimeout(() => g.classList.remove(k, "fx-mosso"), ms);
  }

  /** Reazioni dei giocatori: chi subisce ``punti`` si dispera, chi li fa gioisce. */
  function reazioni(squadraCheSegna, punti) {
    if (punti <= 0) return;
    const livello = punti >= 7 ? 4 : punti >= 4 ? 3 : punti >= 2 ? 2 : 1;
    const dur = 1800 + livello * 500;
    for (let s = 0; s < 4; s++) {
      const { rect: r, el: g } = avatarDi(s);
      if (!r) continue;
      const vittima = squadra(s) !== squadraCheSegna;
      const box = el("div", { class: "fx-reazione", style: `left:${r.left + r.width / 2}px;top:${r.top - 4}px;--dur:${dur}ms;--k:${1 + livello * 0.25}` });
      if (vittima) {
        const pool = REAZ[livello];
        box.append(el("span", { class: "fx-emo" }, pesca(pool.emo)));
        if (livello >= 2 || Math.random() < 0.5) box.append(el("span", { class: "fx-bolla" }, pesca(pool.frasi)));
        const cls = livello >= 4 ? "spazzato" : livello >= 3 ? "arrabbiato" : "triste";
        if (g && cls === "spazzato") g.style.setProperty("--fx-vx", (Math.random() < 0.5 ? -1 : 1) * 160 + "px");
        if (!ridotto()) umore(g, cls, dur);
      } else if (livello >= 2) {
        box.append(el("span", { class: "fx-emo" }, pesca(GIOIA)));
        if (!ridotto()) umore(g, "felice", 1600);
      } else continue;
      layer().append(box);
      setTimeout(() => box.remove(), dur + 100);
    }
    const miaSq = squadra(C.io);
    if (livello >= 3) suona(squadraCheSegna === miaSq ? "festa" : "triste");
  }

  /** Effetti di contorno proporzionati ai punti fatti con una giocata. */
  function clamore(punti, x, y) {
    if (punti >= 2) coriandoli(20 * punti, 2200);
    if (punti >= 3) { flash("#fff", 300); terremoto(bersaglioScossa()); onda(x, y, "#ffd23f", 20); }
    if (punti >= 5) { raggi("rgba(255,210,63,.35)", 2400); fuochi(4); }
  }

  async function anima(e) {
    switch (e.tipo) {
      case "nuova_mano":
        toast(`Mano ${e.mano} — mazziere ${nome(e.mazziere)}`);
        break;
      case "distribuzione": {
        suona("distribuzione");
        const da = rectPosto(e.mazziere);
        const voli = [];
        const w = 34, h = w / RAPPORTO();
        for (let i = 0; i < 4; i++) {
          const s = (e.mazziere + 1 + i) % 4;
          const a = rectPosto(s);
          voli.push(vola(dorso(), centrata(da, w, h), centrata(a, w, h), { durata: 380, ritardo: i * 90 }));
        }
        await Promise.all(voli);
        rivelaTutti();
        assesta();
        break;
      }
      case "inizio_giocate":
        if (mio(e.corrente)) suona("turno");
        break;
      case "scambio_carta":
        privati[e.seat] = e.carta;
        break;
      case "scambio": {
        const priv = privati[e.seat];
        delete privati[e.seat];
        const da = rectPosto(e.seat), a = rectPosto(e.a);
        const w = 56, h = w / RAPPORTO();
        let src = centrata(da, w, h);
        const dst = centrata(a, w, h);
        const vis = priv ? carta(priv) : dorso();
        if (priv && mio(e.seat)) {
          const v = F && F.carte.get(priv);
          if (v) src = v.r;
        }
        assesta("mano");
        if (e.veloce) {
          suona("fuoco");
          scuoti(bersaglioScossa());
          banner("Passa veloce!", "fx-veloce", nome(e.seat), 1300);
          await vola(vis, src, dst, { durata: 650, fiamme: true, ruota: 720 });
        } else {
          suona("passa");
          await vola(vis, src, dst, { durata: 420 });
        }
        break;
      }
      case "scambio_ricevuta": {
        if (!mio(e.seat)) break;
        const da = rectPosto(e.da);
        const w = 56, h = w / RAPPORTO();
        const src = centrata(da, w, h);
        const vero = tutti(`.carte-mano [data-carta="${e.carta}"]`)[0];
        let dst = vero && vero.getBoundingClientRect();
        if (!visibile(dst)) {
          const m = rectPosto(e.seat);
          const cw = dimCarta().w * 1.2;
          dst = rettangolo(m.left + m.width / 2 - cw / 2, m.top + 8, cw, cw / RAPPORTO());
        }
        suona("passa");
        assesta("mano");
        await vola(carta(e.carta), src, dst, { durata: 520, ruota: 360 });
        rivela(e.carta);
        scintille(dst.left + dst.width / 2, dst.top + dst.height / 2, 14, "#5ad1ff");
        toast(`${nome(e.da)} ti ha passato: ${nomeCarta(e.carta)}`);
        break;
      }
      case "bazzica":
        (e.carte || []).forEach(rivela);
        await animaBazzica(e);
        break;
      case "striscia": {
        const r = rectPosto(e.verso);
        const cen = centro(rectTavola());
        const c = centro(r);
        // la carta viene strisciata sul panno tra la mano del giocatore precedente e il centro
        const x = c.x + (cen.x - c.x) * 0.35, y = c.y + (cen.y - c.y) * 0.35;
        suona("striscia");
        cartaStrisciata(dorso(), x, y);
        fumetto(avatarDi(e.seat).rect, "😤");
        break;
      }
      case "tempo_scaduto":
        if (e.fase === "scambio") toast("⏱ Tempo scaduto: chi non aveva scambiato ha passato una carta a caso");
        else toast(`⏱ Tempo scaduto: per ${nome(e.seat)} gioca il computer`);
        break;
      case "frase": {
        suona("palo");
        fumetto(avatarDi(e.seat).rect, e.testo);
        await sleep(300);
        break;
      }
      case "che_palo": {
        suona("palo");
        fumetto(avatarDi(e.seat).rect, "Che Palo!");
        await sleep(500);
        break;
      }
      case "giocata":
        await animaGiocata(e);
        break;
      case "carte_residue": {
        const codici = e.carte || [];
        const els = codici.map(assicuraFantasma);
        await lampeggia(els, "fx-evidenzia", 400);
        await volaAlleTavole(codici, e.squadra, 450, 50);
        assesta("tavola");
        await carteBuone(e.squadra, codici);
        break;
      }
      case "fine_mano": {
        suona("fineMano");
        const rie = e.riepilogo || {};
        for (let t = 0; t < 2; t++) {
          const sq = (rie.squadre || [])[t];
          if (!sq) continue;
          if (sq.napola_len === 10 && sq.napola) await animaNapoleone(t);
          else if (sq.napola) await animaNapola(t, sq);
        }
        await sleep(300);
        break;
      }
      case "fine_partita":
        suona("vittoria");
        break;
      default:
        break;
    }
  }

  async function animaBazzica(e) {
    const r = rectPosto(e.seat);
    const c = centro(r);
    const chi = nome(e.seat);
    let sotto = `${chi} — ${NOMI_TIPO[e.tipo_bazzica] || e.tipo_bazzica}, ${e.scope} Scope`;
    if (e.sette != null) sotto += ` (7 bello = ${e.sette})`;
    const carteBaz = () => (e.carte || []).map((x) => {
      const ce = carta(x);
      if (x === "denari_07" && e.sette != null) ce.append(el("span", { class: "fx-valore-7b" }, e.sette));
      return ce;
    });
    const W = window.innerWidth, H = window.innerHeight;
    const scossa = bersaglioScossa();
    if (e.scope <= 2) {
      // 2: un bel colpo, niente di più
      suona("bazzica2");
      scintille(c.x, c.y, 30, "#ff8be0");
      reazioni(e.squadra, 2);
      await banner("BAZZICA!", "fx-bazzica fx-b2", sotto, 1800);
    } else if (e.scope === 3) {
      // 3: lampo, coriandoli, scossone
      suona("bazzica3");
      flash("#ffd1f0", 350);
      scintille(c.x, c.y, 50, "#ff8be0");
      coriandoli(70, 2400);
      scuoti(scossa);
      reazioni(e.squadra, 3);
      await banner("BAZZICA!", "fx-bazzica fx-b3", sotto, 2000);
    } else if (e.scope <= 5) {
      // 5: buio, raggi, carte giganti, onda d'urto, terremoto
      suona("bazzica5");
      buio(3200);
      raggi("rgba(255,79,163,.4)", 3000);
      carteGiganti(carteBaz(), 2800);
      await sleep(450);
      onda(W / 2, H / 2, "#ff4fa3", 35);
      terremoto(scossa);
      coriandoli(120, 2800);
      reazioni(e.squadra, 5);
      await banner("SUPER BAZZICA!", "fx-bazzica fx-b5", sotto, 2400);
      await sleep(600);
    } else {
      // 7: l'apocalisse
      suona("tamburo");
      buio(6800);
      await sleep(1200);
      for (let i = 0; i < 4; i++) setTimeout(() => { fulmine(); flash("#e6dcff", 200); if (i % 2 === 0) suona("tuono"); }, i * 260);
      await sleep(900);
      suona("bazzica7");
      raggi("rgba(255,255,255,.35)", 4500);
      carteGiganti(carteBaz(), 4200, true);
      await sleep(700);
      onda(W / 2, H / 2, "#fff", 45);
      setTimeout(() => onda(W / 2, H / 2, "#ff4fa3", 45), 180);
      setTimeout(() => onda(W / 2, H / 2, "#ffd23f", 45), 360);
      flash("#fff", 500);
      terremoto(scossa);
      setTimeout(() => terremoto(scossa), 800);
      crepe(W / 2, H / 2, Math.min(W, H) * 0.45);
      const cs = e.carte && e.carte.length ? e.carte : ["denari_07"];
      pioggiaCarte((i) => (i % 3 ? dorso() : carta(cs[i % cs.length])), 60, 2600);
      coriandoli(220, 3400);
      fuochi(10, 250);
      reazioni(e.squadra, 7);
      banner("BAZZICA DA SETTE!!!", "fx-bazzica fx-b7", sotto, 3200);
      await sleep(3600);
    }
  }

  const denari = (i) => `denari_${String(i + 1).padStart(2, "0")}`;

  async function animaNapola(t, sq) {
    suona("napola");
    const n = Math.max(1, Math.min(10, sq.napola_len || 3));
    const carte = Array.from({ length: n }, (_, i) => carta(denari(i)));
    const box = await parataDenari(carte, 180, () => suona("moneta"));
    scintille(window.innerWidth / 2, window.innerHeight / 2, 40, "#ffe066");
    reazioni(t, Math.min(6, n));
    await banner(`${String(sq.napola_nome || "Napola").toUpperCase()}!`, "fx-napola", `${nomeSq(t)} — ${sq.napola} punti`, 2000);
    box.remove();
  }

  async function animaNapoleone(t) {
    // 1) tutte le carte di denari sfilano, una a una, ad arco
    buio(9000);
    const carte = Array.from({ length: 10 }, (_, i) => carta(denari(i)));
    const box = await parataDenari(carte, 300, (i) => { suona("moneta"); if (i === 9) flash("#fff3a0", 300); });
    await sleep(400);
    // 2) incoronazione e festa grande
    suona("napoleone");
    const cor = el("div", { class: "fx-corona" }, "👑");
    layer().append(cor);
    setTimeout(() => cor.remove(), 3600);
    raggi("rgba(255,215,0,.45)", 5000);
    onda(window.innerWidth / 2, window.innerHeight / 2, "#ffd23f", 40);
    banner("NAPOLEONE!", "fx-napola fx-b7", `${nomeSq(t)} prende tutti i denari — 10 punti`, 3400);
    await sleep(700);
    suona("festa");
    coriandoli(260, 3600);
    fuochi(16, 220);
    pioggiaCarte((i) => carta(denari(i % 10)), 40, 2600);
    reazioni(t, 10);
    await sleep(3800);
    box.remove();
  }

  async function animaGiocata(e) {
    const presa = e.presa || [];
    const src = rectSorgente(e.carta, e.seat);
    const vero = nascostoInTavola(e.carta);         // la carta resta in tavola: è già nel DOM, nascosta
    let dst = vero && vero.getBoundingClientRect();
    if (!visibile(dst)) dst = rectCartaTavola();
    const chi = nome(e.seat);
    assesta("mano");

    // ---------------- GRANDE: la carta precipita come un macigno
    if (e.grande) { await animaGrande(e, chi, dst); return; }

    // ---------------- PICCOLA: tutto velocissimo
    if (e.piccola) { await animaPiccola(e, chi, src, dst); return; }

    if (!presa.length) assesta("tavola");
    suona("carta");
    await vola(carta(e.carta), src, dst, { durata: 420 });
    if (!presa.length) {
      if (vero) rivela(e.carta); else posaFantasma(e.carta, dst);
      return;
    }
    const giocata = posaFantasma(e.carta, dst);
    const prese = presa.map(assicuraFantasma);
    await lampeggia([giocata, ...prese], "fx-evidenzia", 450);
    const tr = rectTavola();
    const c = centro(tr);
    const punti = (e.girate || []).length;
    if (e.scopa) {
      // la spazzola passa sul tavolo e "spazza" le carte nel mazzo delle prese
      suona("scopa");
      const spazza = spazzola(tr, 900);
      await sleep(250);
      await volaAlleTavole([e.carta, ...presa], e.squadra, 480);
      assesta("tavola");
      await spazza;
      scintille(c.x, c.y, 30 + punti * 10);
      clamore(punti, c.x, c.y);
      reazioni(e.squadra, punti);
      await bannerLettere("SCOPA!", punti > 1 ? `${chi} — ${punti} Scope!` : chi, punti >= 3 ? "fx-grossa" : "");
      await effettiFinali(e);
      return;
    }
    await volaAlleTavole([e.carta, ...presa], e.squadra, 420);
    assesta("tavola");
    if (punti) {
      clamore(punti, c.x, c.y);
      reazioni(e.squadra, punti);
    }
    await effettiFinali(e);
  }

  /** Le carte (fantasmi in tavola) volano nel mazzo delle prese della squadra. */
  async function volaAlleTavole(codici, squadraT, durata, passo = null) {
    const dest = rectPrese(squadraT);
    const voli = codici.map((code, i) => {
      const g = assicuraFantasma(code);
      const r = g.getBoundingClientRect();
      S.fantasmi.delete(code);
      g.remove();
      return vola(carta(code), r, dest, { durata, ritardo: i * (passo != null ? passo : Math.min(45, durata / 8)) });
    });
    suona("presa");
    await Promise.all(voli);
  }

  /** Prese "buone": sette bello, re bello e i sette (primiera) hanno un piccolo momento di gloria. */
  async function carteBuone(squadraT, carte) {
    const buone = [];
    if (carte.includes("denari_07")) buone.push(["denari_07", "SETTE BELLO!", "settebello", "oro"]);
    if (carte.includes("denari_10")) buone.push(["denari_10", "RE BELLO!", "rebello", "re"]);
    for (const c of carte) if (val(c) === 7 && c !== "denari_07") buone.push([c, "UN SETTE!", "sette", "primiera"]);
    if (!buone.length) return;
    const dest = rectPrese(squadraT);
    for (const [c, testo, suono, stile] of buone.slice(0, 3)) {
      suona(suono);
      await cartaDiGloria(carta(c), testo, stile, dest, buone.length > 1 ? 850 : 1150);
    }
    if (buone.some((b) => b[3] !== "primiera")) reazioni(squadraT, 1);
  }

  async function effettiFinali(e) {
    if (e.presa && e.presa.length) await carteBuone(e.squadra, [e.carta, ...e.presa]);
    if (e.assi && e.assi.length) {
      suona("asso");
      await banner(e.assi.length > 1 ? `${e.assi.length} ASSI!` : "ASSO!", "fx-asso", "Scopa (Avogadro)", 1200);
    }
    if (e.credito_pagato) toast(`${nomeSq(e.squadra)}: riscossi ${e.credito_pagato} di credito Bazzica`);
    if (e.girate && e.girate.length) {
      const r = rectPrese(e.squadra);
      scintille(r.left + r.width / 2, r.top + r.height / 2, 10);
    }
  }

  async function animaPiccola(e, chi, src, dst) {
    const presa = e.presa || [];
    const target = presa.length ? assicuraFantasma(presa[0]) : null;
    const tr = target ? target.getBoundingClientRect() : dst;
    const c = centro(tr);
    suona("piccola");
    lineeVelocita(c.x, c.y);
    saetta(centro(src), c);
    await vola(carta(e.carta), src, tr, { durata: 130 });
    sbuffi(c.x, c.y, 12);
    posaFantasma(e.carta, rettangolo(tr.left + 10, tr.top - 6, tr.width, tr.height));
    if (target) target.classList.add("fx-evidenzia");
    const zip = bannerZip("PICCOLA!", `${chi} — lampo!`);
    reazioni(e.squadra, (e.girate || []).length);
    saetta(c, centro(rectPrese(e.squadra)));
    await volaAlleTavole([e.carta, ...presa], e.squadra, 170);
    assesta("tavola");
    await zip;
    await effettiFinali(e);
  }

  async function animaGrande(e, chi, dst) {
    const presa = e.presa || [];
    const prese = presa.map(assicuraFantasma);
    // punto d'impatto: sopra le carte prese, se ci sono
    let bersaglio = dst;
    if (prese.length) {
      const r = prese[0].getBoundingClientRect();
      bersaglio = rettangolo(r.left, r.top, r.width, r.height);
    }
    vignetta();
    suona("grandeCaduta");
    prese.forEach((x) => x.classList.add("fx-evidenzia"));
    const { box, c } = await macigno(carta(e.carta), bersaglio, 620);
    // IMPATTO
    suona("grande");
    const scossa = bersaglioScossa();
    terremoto(scossa);
    crepe(c.x, c.y, Math.max(140, bersaglio.width * 2.2));
    polvere(c.x, c.y + bersaglio.height * 0.35, 22);
    scintille(c.x, c.y, 26, "#ffcc33");
    prese.forEach((x, i) => { x.style.transition = "transform .25s"; x.style.transform = `translateY(10px) rotate(${i % 2 ? 7 : -7}deg) scale(.92)`; });
    bannerPesante("GRANDE!", `${chi} — ${presa.length + 1} Scope`);
    // gli avversari accusano il colpo (più è grande, più si disperano)
    const punti = (e.girate || []).length;
    if (!ridotto()) {
      for (const sx of [0, 1, 2, 3].filter((x) => squadra(x) !== e.squadra)) umore(avatarDi(sx).el, "abbattuto", 2600);
    }
    reazioni(e.squadra, punti + 1);
    if (punti >= 3) { setTimeout(() => terremoto(scossa), 350); onda(c.x, c.y, "#3a1d0b", 25); }
    if (punti >= 4) { pioggiaCarte(() => dorso(), 8 * punti, 1600); setTimeout(() => suona("tuono"), 300); }
    await sleep(1100);
    // la carta-macigno e il bottino scivolano lentamente, pesanti, nel mazzo delle prese
    const gr = box.getBoundingClientRect();
    box.remove();
    posaFantasma(e.carta, gr);
    await volaAlleTavole([e.carta, ...presa], e.squadra, 700);
    assesta("tavola");
    await sleep(500);
    await effettiFinali(e);
  }

  // =================================================================== coda degli eventi
  const IMMEDIATI = new Set(["frase", "che_palo", "striscia"]);
  let coda = Promise.resolve();
  let pendenti = 0;

  async function esegui(e) {
    if (!e || typeof e !== "object") return;
    try {
      await Promise.race([anima(e), sleep(20000)]);
    } catch (err) {
      console.warn("[BazzicaFx] evento", e && e.tipo, err);
    }
  }

  /**
   * Accoda una lista di eventi e li anima in ordine (un lotto aspetta i precedenti).
   * Va chiamata subito DOPO aver aggiornato il DOM; istantanea() va chiamata subito PRIMA.
   */
  function eventi(lista, ctx) {
    try {
      const f = foto && performance.now() - foto.t < 5000 ? foto : null;
      foto = null;
      const c = normalizzaCtx(ctx);
      const l = Array.isArray(lista) ? lista.filter((x) => x && typeof x === "object") : [];
      // esclamazioni e "striscia": subito, senza aspettare le altre animazioni
      if (l.length && l.every((x) => IMMEDIATI.has(x.tipo))) {
        return dopoFrame().then(async () => { const prima = C; C = c; try { await Promise.all(l.map(esegui)); } finally { C = prima; } });
      }
      pendenti++;
      if (f) daPreparare.push(f);
      const pronto = dopoFrame().then(() => {
        C = c;
        daPreparare = daPreparare.filter((x) => x !== f);
        try { precarica(); prepara(f); } catch (err) { console.warn("[BazzicaFx] prepara", err); }
      });
      coda = coda.then(() => pronto).then(async () => {
        C = c;
        for (const e of l) await esegui(e);
      }).catch((err) => console.warn("[BazzicaFx]", err)).finally(() => {
        pendenti--;
        if (!pendenti) { try { pulisci(); } catch (err) { console.warn("[BazzicaFx] pulisci", err); } }
      });
      return coda;
    } catch (err) {
      console.warn("[BazzicaFx] eventi", err);
      return Promise.resolve();
    }
  }

  const sicuro = (f) => function () { try { return f.apply(null, arguments); } catch (err) { console.warn("[BazzicaFx]", err); } };

  window.BazzicaFx = {
    istantanea,
    eventi,
    suona: sicuro(suona),
    setMuto: sicuro(setMuto),
    isMuto,
    tic: sicuro(() => suona("tic")),
    turno: sicuro(() => suona("turno")),
    sblocca: sicuro(sblocca),
    toast: sicuro(toast),
  };
})();
