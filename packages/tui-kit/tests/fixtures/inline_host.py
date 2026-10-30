"""Owned native inline fixture transitions through the shared VT harness."""
import os,sys,time
from inline_pty import Host

# The shared choose_many prompt: context above a short list, the minimum notice, then marks.
h = Host(args=[sys.argv[1], '--inline', '--component', 'inline-composition', '--scenario', 'choose-many'])
try:
    h.wait('Acme api')
    assert 'Pick services to deploy' in h.screen(),h.screen()
    h.send(b'\r','press space to mark at least 1')
    assert h.child.poll() is None
    os.write(h.master,b' '); h.drain(.2)
    os.write(h.master,b'\r'); h.finish()
    assert 'services → api' in h.all_text(),h.all_text()
finally: h.close()

h = Host('main')
try:
    h.wait('Deploy'); h.send(b'\x1b[20~','Deploy')  # F9 host output
    h.wait('host: background output arrived')
    assert '~/acme-api $ acme preview' in h.all_text(),h.all_text()
    assert h.all_text().index('plan · 3 services') < h.all_text().index('host: background output arrived')
    h.resize(18,4); h.wait('small | esc cancel'); os.write(h.master,b'\r'); h.drain(.2)
    assert h.child.poll() is None
    h.resize(80,24); h.wait('Deploy API only')
    h.send(b'\x0c','Deploy API only')  # owned redraw
    h.send(b'\x1b[23~','foreground host output')  # F11 release/resume, with cursor report
    h.wait('Deploy API only')
    os.write(h.master,b'\x03'); h.finish()
    assert 'cancelled' in h.all_text(),h.all_text()
    assert h.output.count(b'host output after inline result')==1
    assert b'~/acme-api $ acme preview' in h.output
finally: h.close()

h = Host('main', report_cursor=False)
try:
    h.wait('no default:'); os.write(h.master,b'\n3\n'); h.finish()
    assert 'cancelled' in h.all_text()
finally: h.close()

h = Host('streaming')
try:
    h.wait('and bump the app version')
    h.wait('actions on')
    def live_span(marker):
        rows=h.screen().splitlines()
        start=next((i for i,row in enumerate(rows) if marker in row),None)
        end=next((i for i,row in enumerate(rows) if 'actions on' in row),None)
        return 0 if start is None or end is None else end-start+1
    assert live_span('edit src/settings.ts')==10,h.screen()
    h.send(b'\x1b[15~','edit src/settings.ts → done')
    deadline=time.monotonic()+10
    while live_span('run tests')!=9:
        assert time.monotonic()<deadline,h.screen()
        h.drain()
    h.send(b'\x18o','sessions'); assert h.saved is not None
    h.resize(80,24); h.wait('sessions')
    h.drain(.1); h.resize(100,32); h.wait('sessions'); h.drain(.1)
    os.write(h.master,b'\x1b[20~'); h.drain(.2)  # output is deferred on alternate screen
    h.send(b'\x1b','and bump the app version'); assert h.saved is None
    h.wait('host: background output arrived')
    h.send(b'!','and bump the app versio!n')
    h.send(b'\x06','find'); os.write(h.master,b'timezone\r'); h.drain(.2)
    assert h.saved is not None,h.screen()
    os.write(h.master,b'\x1b'); h.wait_mode(False); h.wait('and bump the app versio!n')
    h.send(b'\x18i','Context'); os.write(h.master,b'\x1b'); h.wait_mode(False); h.wait('and bump the app versio!n')
    # Clear the retained draft through the component, then idle Ctrl+C closes safely.
    os.write(h.master,b'\x1b[H\x0b'); h.drain(.2); os.write(h.master,b'\x03'); h.drain(.2)
    if h.child.poll() is None: os.write(h.master,b'\x03')
    h.finish()
    assert 'acme · Adding the timezone' in h.all_text(),h.all_text()
    assert h.output.count(b'host: background output arrived')==1
finally: h.close()

h = Host('main')
try:
    h.wait('Deploy'); os.write(h.master,b'\x1b[B')
    h.send(b'\x1b[20~','host: background output arrived')
    os.write(h.master,b'\r'); h.finish()
    assert 'Deploy API only' in h.all_text(),h.all_text()
    assert h.output.count('preview → Deploy API only'.encode())==1,h.output[-1000:]
    assert '~/acme-api $ acme preview' in h.all_text(),h.all_text()
finally: h.close()
