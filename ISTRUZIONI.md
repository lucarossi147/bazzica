# Bazzica — istruzioni

Si gioca dal browser: **https://lucarossi147.github.io/bazzica/**

Non c'è un server di gioco. Ogni telefono o PC esegue il motore delle regole e i giocatori
si collegano direttamente tra loro (WebRTC). Per trovarsi usano dei relay pubblici Nostr,
che vedono passare solo messaggi cifrati con il codice del tavolo (la chiave si ricava dal codice
con scrypt, quindi indovinare i codici guardando i relay costa molto).

## Giocare
1. Apri la pagina, scrivi il tuo nome e scegli avatar e carte (Disegnate o Piacentine).
2. **Crea un tavolo** e manda agli amici il link o il QR (pulsante ⚙), oppure
   scrivi il codice di 10 lettere di un tavolo esistente ed **Entra** (i codici delle versioni
   precedenti non valgono più).
3. Sedetevi ai posti; quelli vuoti si possono riempire con dei bot. Chi ha creato il tavolo
   può liberare i posti degli altri. Le regole (variante, Margherita Hack, timer, attesa a fine
   mano, nomi delle squadre) le sceglie chi ha creato il tavolo; se è uscito, la regia.
4. **Inizia la partita** (chi ha creato il tavolo o chi è seduto) quando tutti e 4 i posti
   sono occupati.

Su iPhone: Condividi → *Aggiungi alla schermata Home* per averla come app a tutto schermo.
I suoni partono dopo il primo tocco sulla pagina.

## Durante la partita
- Tocca una carta: compaiono le prese possibili; tocca l'opzione per giocare.
- **Bazzica!** e **Che palo!** compaiono quando sono possibili.
- 💬 apre le frasi veloci (una ogni 3 secondi al massimo).
- **Striscia**: trascina una carta per mettere fretta al giocatore prima di te.
- Il pulsante ⚙ contiene invito (link e QR), scelta delle carte, suoni e **Interrompi**.
- Se un giocatore seduto si scollega la partita va in pausa finché non riapre il link dallo
  stesso browser (nome e identità sono ricordati sul dispositivo). Dopo un minuto chi è seduto
  può **sostituirlo con un bot**; quando torna, **Riprendi il posto** glielo restituisce.
- A fine mano il riepilogo aspetta tutti al massimo il tempo scelto in *Attesa a fine mano*
  (30 secondi; "nessun limite" aspetta per sempre).
- Ogni identità gioca da un posto solo: se apri lo stesso tavolo in un'altra scheda (o ricarichi
  la pagina), vale l'ultima aperta e quella vecchia lo dice, con **Gioca qui** per riprenderlo.
- Se chi fa da "regia" (il tavolo che ordina le mosse) esce, un altro giocatore collegato
  prende il suo posto senza interrompere la partita.
  Serve la maggioranza dei giocatori collegati: se la rete si spezza in due, solo la parte più
  grande continua; l'altra mostra "Non riesco a raggiungere la regia".
- Cambiare scheda o app per meno di mezzo minuto non interrompe nulla: chi fa da regia continua
  a far giocare i bot anche con la pagina nascosta.

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

### Server TURN
Tra reti diverse (dati mobili, NAT dell'operatore) i telefoni spesso non si collegano
direttamente: serve un server TURN che faccia da ponte. Nessuna credenziale sta nel codice:
impostala nei segreti del repository (*Settings → Secrets and variables → Actions*):
`BAZZICA_TURN_URLS` (gli indirizzi `turn:` e `turns:` separati da spazi), `BAZZICA_TURN_USERNAME`
e `BAZZICA_TURN_CREDENTIAL`, per esempio quelli di un account gratuito Metered. Senza segreti
l'app usa solo STUN. Ognuno può anche impostare un proprio server nella schermata di
collegamento (*Server TURN*). Le credenziali finiscono comunque nell'app pubblicata: per non
farsi consumare la quota si possono generare credenziali a tempo con una piccola funzione
serverless (Cloudflare Worker o l'API di Metered).

## Scelte di regolamento concordate
- Il 7 bello a cui si assegna 1 non piglia mai tutto (vale 1).
- Il credito di Bazzica si riscuote dopo Scope/Piccole/Grandi/assi, su tutte le carte coperte della presa, compresa quella giocata.
- "Che Palo!": si può esclamare quando si vuole (i bot lo fanno solo se la mano è davvero a una carta da una Bazzica).
- Scambio: la carta del compagno arriva solo quando entrambi i compagni hanno passato.
- Avogadro: anche l'asso giocato che fa presa va a faccia in su.
- Einstein: qualsiasi tris che contiene il 7 bello vale 5 Scope.
