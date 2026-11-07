"""End-to-end browser test of the web app: two players in separate Edge contexts play a full
game over real Nostr relays and WebRTC
(A creates the table, B joins with the invite link, two bots fill the other seats).

Setup (once):  py -m venv .venv && .venv/Scripts/python -m pip install playwright
Serve a bundle under /bazzica/ (dx bundle --platform web --release, then serve a folder
that contains dist/public renamed to bazzica/ on port 8080), then run:
    .venv/Scripts/python tools/prova_web.py <screenshot dir> [url]
"""
import sys, time
from playwright.sync_api import sync_playwright

URL = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:8080/bazzica/"
OUT = sys.argv[1]
TELEFONO = {"width": 390, "height": 844}


def log_console(nome, page):
    page.on("console", lambda m: print(f"[{nome} {m.type}] {m.text}"[:300]))
    page.on("pageerror", lambda e: print(f"[{nome} pageerror] {e}"[:300]))


def entra(page, nome):
    page.fill("input[placeholder=Nome]", nome)


def gioca_un_passo(page):
    """Does one thing if possible; returns True if it acted."""
    for testo in ("Prosegui",):
        b = page.get_by_role("button", name=testo)
        if b.count() and b.first.is_enabled():
            b.first.click()
            return True
    opz = page.locator("button.opzione")
    if opz.count():
        opz.first.click()
        return True
    carte = page.locator(".carte-mano img.cliccabile")
    if carte.count():
        carte.first.click()
        return True
    return False


with sync_playwright() as p:
    browser = p.chromium.launch(channel="msedge", headless=True)
    ca = browser.new_context(viewport=TELEFONO, device_scale_factor=2)
    cb = browser.new_context(viewport=TELEFONO, device_scale_factor=2)
    for c in (ca, cb):
        c.add_init_script("localStorage.setItem('bazzica.debug', '1')")
    a, b = ca.new_page(), cb.new_page()
    log_console("A", a)
    log_console("B", b)

    a.goto(URL)
    entra(a, "Anna")
    a.get_by_role("button", name="Crea un tavolo").click()
    a.wait_for_selector(".codice", timeout=30000)
    codice = a.inner_text(".codice")
    print("table", codice)
    a.screenshot(path=f"{OUT}/1-stanza-a.png")

    b.goto(URL + "#" + codice)
    entra(b, "Bruno")
    b.get_by_role("button", name="Entra").click()
    t0 = time.time()
    try:
        b.wait_for_selector(".codice", timeout=40000)
    except Exception:
        b.screenshot(path=f"{OUT}/x-b-attesa.png")
        raise
    print(f"B joined in {time.time() - t0:.1f}s")

    # seats: A north (0), B south (2), bots east and west
    a.locator(".area-n").get_by_role("button", name="Siediti").click()
    b.wait_for_timeout(500)
    b.locator(".area-s").get_by_role("button", name="Siediti").click()
    a.wait_for_timeout(500)
    a.locator(".area-e").get_by_role("button", name="Bot").click()
    a.wait_for_timeout(300)
    b.locator(".area-o").get_by_role("button", name="Bot").click()
    a.wait_for_timeout(800)
    b.screenshot(path=f"{OUT}/2-stanza-b.png")
    a.get_by_role("button", name="Inizia la partita").click()
    a.wait_for_selector(".partita", timeout=10000)
    b.wait_for_selector(".partita", timeout=10000)
    a.wait_for_timeout(500)
    a.screenshot(path=f"{OUT}/3-partita-a.png")

    fatto = {"opzioni": False, "fine_mano": False}
    t0 = time.time()
    while time.time() - t0 < 600:
        if a.get_by_role("button", name="Rivincita").count() and b.get_by_role("button", name="Rivincita").count():
            break
        mosso = False
        for nome, page in (("A", a), ("B", b)):
            if not fatto["opzioni"] and page.locator("button.opzione").count():
                page.screenshot(path=f"{OUT}/4-opzioni-{nome}.png")
                fatto["opzioni"] = True
            if not fatto["fine_mano"] and page.get_by_role("button", name="Prosegui").count():
                page.screenshot(path=f"{OUT}/5-fine-mano-{nome}.png")
                fatto["fine_mano"] = True
            mosso |= gioca_un_passo(page)
        a.wait_for_timeout(150 if mosso else 400)
    else:
        a.screenshot(path=f"{OUT}/x-timeout-a.png")
        b.screenshot(path=f"{OUT}/x-timeout-b.png")
        print("TIMEOUT")
        sys.exit(1)
    print(f"game over in {time.time() - t0:.0f}s")
    a.screenshot(path=f"{OUT}/6-fine-a.png")
    b.screenshot(path=f"{OUT}/6-fine-b.png")
    browser.close()
