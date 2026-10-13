"""Exercise the public inline height API under an owned cursor-reporting terminal."""
import os,sys,time
from inline_pty import Host

h = Host(args=[sys.argv[1], '--exact', 'inline_composition::height_changes_preserve_output_and_defer_borrowed_screen_updates', '--nocapture', '--test-threads=1'], env_extra={'CANONIC_TUI_INLINE_HEIGHT_CHILD':'1'})
def span(name):
    lines=h.screen().splitlines()
    top=next((i for i,line in enumerate(lines) if 'LIVE '+name in line),None)
    bottom=next((i for i,line in enumerate(lines) if 'FOOT '+name in line),None)
    return 0 if top is None or bottom is None else bottom-top+1
try:
    h.wait('FOOT short'); assert span('short')==4,h.screen()
    h.send(b'g','FOOT grown'); assert span('grown')==30,h.screen()
    assert 'BEFORE SENTINEL' in h.all_text(),h.all_text()
    h.send(b's','FOOT shrunk'); assert span('shrunk')==4,h.screen()
    assert 'FOOT grown' not in h.screen(),h.screen()
    h.send(b'z','FOOT rejected'); assert span('rejected')==4,h.screen()
    h.send(b'a','FOOT alternate'); h.wait_mode(True)
    saved=[''.join(row) for row in h.saved[0]]
    h.send(b'd','FOOT deferred'); assert span('deferred')==32,h.screen()
    assert saved==[''.join(row) for row in h.saved[0]]
    h.send(b'r','FOOT returned'); h.wait_mode(False); assert span('returned')==9,h.screen()
    h.send(b'p','FOOT late'); assert span('late')==9,h.screen()
    h.resize(60,8); h.wait('FOOT late')
    # Wait for the new region, rather than matching its pre-resize stale label.
    deadline=time.monotonic()+10
    while span('late')!=6:
        assert time.monotonic()<deadline,h.screen()
        h.drain()
    h.send(b'f','FOREGROUND SENTINEL'); h.wait('FOOT resumed'); assert span('resumed')==6,h.screen()
    h.resize(100,32)
    deadline=time.monotonic()+10
    while 'FOOT resumed' not in h.screen() or span('resumed')!=9:
        assert time.monotonic()<deadline,h.screen()
        h.drain()
    os.write(h.master,b'x'); h.finish()
    for text in ['BEFORE SENTINEL','LATE SENTINEL','FOREGROUND SENTINEL','AFTER SENTINEL']:
        assert text in h.all_text(),(text,h.all_text())
finally:h.close()
