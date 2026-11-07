# Bazzica — istruzioni

Si gioca dal browser: **https://lucarossi147.github.io/bazzica/**

Non c'è un server di gioco. Ogni telefono o PC esegue il motore delle regole e i giocatori
si collegano direttamente tra loro (WebRTC). Per trovarsi usano dei relay pubblici Nostr,
che vedono passare solo messaggi cifrati con il codice del tavolo.

## Giocare
1. Apri la pagina, scrivi il tuo nome e scegli avatar e carte (Disegnate o Piacentine).
2. **Crea un tavolo** e manda agli amici il link o il QR (pulsante ⚙), oppure
   scrivi il codice di 8 lettere di un tavolo esistente ed **Entra**.
3. Sedetevi ai posti; quelli vuoti si possono riempire con dei bot. Chi ha creato il tavolo
   può liberare i posti degli altri. Le regole (variante, Margherita Hack, timer, nomi delle
   squadre) si scelgono nella stanza.
4. **Inizia la partita** quando tutti e 4 i posti sono occupati.

Su iPhone: Condividi → *Aggiungi alla schermata Home* per averla come app a tutto schermo.
I suoni partono dopo il primo tocco sulla pagina.

## Durante la partita
- Tocca una carta: compaiono le prese possibili; tocca l'opzione per giocare.
- **Bazzica!** e **Che palo!** compaiono quando sono possibili.
- 💬 apre le frasi veloci (una ogni 3 secondi al massimo).
- **Striscia**: trascina una carta per mettere fretta al giocatore prima di te.
- Il pulsante ⚙ contiene invito (link e QR), scelta delle carte, suoni e **Interrompi**.
- Se un giocatore seduto si scollega la partita va in pausa finché non riapre il link dallo
  stesso browser (nome e identità sono ricordati sul dispositivo).
- Se chi fa da "regia" (il tavolo che ordina le mosse) esce, un altro giocatore collegato
  prende il suo posto senza interrompere la partita.

## Sviluppo

### Struttura
```
crates/bazzica-core/   regole, punteggio e bot (Rust), verificati sulle tracce
crates/bazzica-net/    protocollo del tavolo, senza rete: log ordinato, elezione della regia
crates/bazzica-rete/   trasporto: WebRTC (matchbox) con segnalazione su Nostr
crates/bazzica-app/    app web (Dioxus → WASM), effetti e suoni in public/fx/
bazzica/               motore di riferimento in Python (regole e bot)
testdata/tracce/       partite registrate dal motore Python: l'oracolo per quello Rust
tools/                 generazione delle carte, export/verifica tracce, test end-to-end
```

### Comandi
```
cargo test --workspace                     # motore, protocollo, trasporto
cargo test -p bazzica-rete -- --ignored    # partita dal vivo tra 4 peer sui relay veri
python -m pytest                           # motore Python (BAZZICA_SIM=10 per fare prima)
python tools/verifica_tracce.py            # il motore Python riproduce le tracce
python tools/esporta_tracce.py             # rigenera le tracce

cd crates/bazzica-app
dx bundle --platform web --release         # (dioxus-cli 0.7.9) → dist/public
```
Per provare in locale servi `dist/public` sotto `/bazzica/` (per esempio copiandolo in
`sito/bazzica` e lanciando `python -m http.server 8080` da `sito`). Per avere più giocatori
nello stesso browser aggiungi `?profilo=a`, `?profilo=b`, … all'indirizzo: ogni profilo ha
un'identità separata. `localStorage["bazzica.debug"] = 1` attiva i log dettagliati.

`tools/prova_web.py` gioca una partita completa tra due browser (Playwright con Edge).

### Pubblicazione
Ogni push su `main` esegue la CI (fmt, clippy, test Rust e Python) e pubblica l'app su
GitHub Pages (`.github/workflows/pages.yml`).

## Scelte di regolamento concordate
- Il 7 bello a cui si assegna 1 non piglia mai tutto (vale 1).
- Il credito di Bazzica si riscuote dopo Scope/Piccole/Grandi/assi, su tutte le carte coperte della presa, compresa quella giocata.
- "Che Palo!": si può esclamare quando si vuole (i bot lo fanno solo se la mano è davvero a una carta da una Bazzica).
- Scambio: la carta del compagno arriva solo quando entrambi i compagni hanno passato.
- Avogadro: anche l'asso giocato che fa presa va a faccia in su.
- Einstein: qualsiasi tris che contiene il 7 bello vale 5 Scope.
