"""Exercise the real headless broker and stdio relay, including saved-state recovery."""
import json
import math
import os
from pathlib import Path
import socket
import select
import subprocess
import sys
from tempfile import TemporaryDirectory
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hissabeth"
WORKSPACE = TemporaryDirectory(dir=ROOT / "target", prefix="transport-workspace-")
DATA = Path(WORKSPACE.name)
if "--workspace-recovery" in sys.argv:
    (DATA / "system.json").write_bytes(b"{ broken saved project\n")

ENV = dict(os.environ, HISSABETH_DATA_DIR=str(DATA))
# Vendor catalogs are not bundled: install only the SCHOTT test fixture, as the
# Glass Catalogs window records a local import, so other vendors stay missing.
(DATA / "glass").mkdir()
(DATA / "glass/SCHOTT.agf").write_bytes((ROOT / "tests/fixtures/glass/SCHOTT.agf").read_bytes())
(DATA / "glass/catalogs.json").write_text(json.dumps(dict(dismissed=True, catalogs=dict(SCHOTT=dict(origin=dict(file=dict(name="SCHOTT.agf")), installed_at=0, glasses=0)))))


def start_engine():
    process = subprocess.Popen([str(BINARY), "--engine"], env=ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError(process.stderr.read().decode())
        if (DATA / "endpoint.json").exists():
            return process
        time.sleep(0.05)
    process.terminate()
    raise TimeoutError("Broker did not publish its endpoint")


engine = start_engine()
relay = subprocess.Popen([str(BINARY), "--mcp"], env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
identifier = 0
last_response_bytes = 0


def exchange(line: str) -> dict:
    global last_response_bytes
    relay.stdin.write(line + "\n")
    relay.stdin.flush()
    if not select.select([relay.stdout], [], [], 20)[0]:
        raise TimeoutError("No MCP response")
    line = relay.stdout.readline()
    last_response_bytes = len(line.encode())
    return json.loads(line)


def request(method, params=None):
    global identifier
    identifier += 1
    message = dict(jsonrpc="2.0", id=identifier, method=method)
    if params is not None:
        message["params"] = params
    response = exchange(json.dumps(message))
    assert response["id"] == identifier, response
    assert "error" not in response, response
    return response["result"]


def tool(name, args=None, error=False):
    result = request("tools/call", dict(name=name, arguments=args if args is not None else {}))
    assert result["isError"] is error, result
    return result if error else json.loads(result["content"][0]["text"])


def malformed_parameters() -> None:
    before: dict = tool("system_get")
    revision: int = before["revision"]
    cases: list[tuple[str, object, str]] = [
        ("system_get", False, "Invalid type"),
        ("paraxial", {"wavelength": 20_001}, "Value outside range"),
        ("paraxial", {"wavelength": "NaN"}, "Invalid type"),
        ("paraxial", {"wavelength": "Infinity"}, "Invalid type"),
        ("batch", {"steps": [None] * 100_001}, "Invalid array length"),
        ("project_save_as", {"path": "", "revision": revision}, "Project path is required"),
        # The relay holds agents to the data directory by default.
        ("project_save_as", {"path": str(DATA.parent / "outside-scope"), "revision": revision}, "File access denied"),
        ("project_open", {"path": str(DATA.parent / "outside-scope.hissabeth.json"), "revision": revision}, "File access denied"),
        ("project_new", {"revision": revision + 1}, "System changed"),
    ]
    for name, args, expected in cases:
        result: dict = tool(name, args, error=True)
        content: dict = result["content"][0]
        assert content["type"] == "text" and expected in content["text"], (name, content)
        assert tool("system_get") == before, (name, expected)

    deep: str = "[" * 1000 + "0" + "]" * 1000
    prefix: str = '{"jsonrpc":"2.0","id":1000,"method":"tools/call","params":'
    prefix += '{"name":"paraxial","arguments":{"wavelength":'
    for value in ("1e400", "-1e400", deep):
        response: dict = exchange(prefix + value + "}}}")
        assert response["id"] is None and response["error"]["code"] == -32700, response
        assert tool("system_get") == before

    report: dict = tool("batch", {
        "revision": revision,
        "steps": [
            {"command": "project_new", "args": {}},
            {"command": "paraxial", "args": {"wavelength": "Infinity"}},
        ],
    })
    assert report["ok"] is False and report["revision"] == revision, report
    assert report["failed"] == {
        "index": 1, "command": "paraxial", "error": "Invalid type at arguments.wavelength",
    }
    assert tool("system_get") == before
    print("PASS: real MCP tool errors, parser rejection, broker liveness, and batch rollback")


def artifacts() -> None:
    before: dict = tool("system_get")
    tool("bench_template", {"name": "michelson", "apply": True, "revision": before["revision"]})
    started: dict = tool("artifact_start", {"kind": "blueprint", "format": "svg"})
    deadline = time.monotonic() + 60
    while (status := tool("artifact_status", {"id": started["id"]}))["state"] in ("queued", "running"):
        assert time.monotonic() < deadline, status
        time.sleep(0.02)
    assert status["state"] == "done" and status["mime"] == "image/svg+xml", status
    result: dict = request("tools/call", dict(name="artifact_fetch", arguments={"id": started["id"]}))
    meta, resource = result["content"]
    assert result["isError"] is False and json.loads(meta["text"])["bytes"] == status["bytes"], meta
    assert resource["type"] == "resource" and resource["resource"]["mimeType"] == "image/svg+xml", resource
    assert "<svg" in resource["resource"]["text"][:200], resource["resource"]["text"][:200]
    denied: dict = tool("artifact_export", {"id": started["id"], "path": str(DATA.parent / "outside-scope.svg")}, error=True)
    assert "File access denied" in denied["content"][0]["text"], denied
    exported: dict = tool("artifact_export", {"id": started["id"], "path": "transport.svg"})
    assert (DATA / "projects" / "transport.svg").stat().st_size == status["bytes"], exported
    print("PASS: artifact start, status, fetch as MCP resource, and scoped export")


def workspace_recovery():
    global engine
    status=tool("workspace_status")
    assert status["state"]=="recovery" and status["source_text"]=="{ broken saved project\n"
    revision=status["revision"];original=(DATA/"system.json").read_bytes()
    tool("workspace_retry", dict(revision=revision + 1), error=True)
    for name,args in [("system_get",{}),("layout",{}),("capabilities",{}),("materials_list",{}),("project_import",dict(format="hissabeth",text="{}",revision=revision))]:
        tool(name,args,error=True)
    for args in [{},dict(revision=revision+1,source=dict(kind="default")),dict(revision=revision,source=dict(kind="default",extra=True)),dict(revision=revision,source=dict(kind="project",format="hissabeth",text="{}"))]:
        tool("workspace_recover",args,error=True)
    assert (DATA/"system.json").read_bytes()==original and tool("workspace_status")==status
    restored=tool("workspace_recover",dict(revision=revision,source=dict(kind="default")))
    assert restored["revision"]==revision+1
    assert Path(restored["preserved_path"]).read_bytes()==original
    snapshot=tool("system_get")
    assert snapshot["revision"]==restored["revision"]
    assert tool("workspace_status")["state"]=="ready"
    assert tool("capabilities")["scope"]["startup_recovery"]
    import copy
    exchange=copy.deepcopy(snapshot["system"]);exchange.update(pupil_mapping="reference_entrance_pupil",object_distance=1e10,temperature=25)
    tool("system_set",dict(system=exchange,revision=snapshot["revision"]))
    geopter=tool("project_export",dict(format="geopter"))["text"]
    engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink()
    (DATA/"system.json").write_bytes(b"another broken source")
    engine=start_engine();status=tool("workspace_status");revision=status["revision"]
    (DATA/"system.json").write_bytes(b"changed externally")
    tool("workspace_recover",dict(revision=revision,source=dict(kind="default")),error=True)
    assert (DATA/"system.json").read_bytes()==b"changed externally"
    retried=tool("workspace_retry",dict(revision=revision))
    assert retried["state"]=="recovery" and retried["revision"]==revision+1 and retried["source_text"]=="changed externally"
    tool("workspace_retry",dict(revision=revision),error=True)
    system=snapshot["system"];system["name"]="Externally repaired optics"
    (DATA/"system.json").write_text(json.dumps(system))
    ready=tool("workspace_retry",dict(revision=revision+1))
    assert ready["state"]=="ready" and ready["revision"]==revision+2
    assert tool("system_get")["system"]["name"]==system["name"]
    for format,text in [("hissabeth",json.dumps(system)),("geopter",geopter)]:
        engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink()
        original=b"\xff\xfe\x00broken"
        (DATA/"system.json").write_bytes(original);engine=start_engine()
        status=tool("workspace_status")
        assert status["state"]=="recovery" and status["source_text"] is None and status["source_bytes"]==len(original)
        result=tool("workspace_recover",dict(revision=status["revision"],source=dict(kind="project",format=format,text=text)))
        assert Path(result["preserved_path"]).read_bytes()==original
        assert tool("system_get")["system"]["name"]==("Externally repaired optics" if format=="hissabeth" else "BK7 singlet")
        assert math.isfinite(tool("paraxial",dict(wavelength=587.5618))["efl"])
    print("PASS: real connected startup recovery; blocked optical writes, strict/stale schemas, byte-exact preservation, native/Geopter/new-system recovery, changed-disk rejection, invalid/valid external retry and same-relay restart")

def project_lifecycle():
    global engine
    current=tool("system_get")
    rich=json.loads(json.dumps(current["system"]))
    rich["name"]="Saved before new";rich["note"]="Prior project notes"
    rich["object_distance"]=200;rich["object_label"]="Previous object";rich["image_label"]="Previous image"
    rich["surfaces"][0]["gap_solve"]=dict(kind="pickup",source=1,scale=.1,offset=0)
    tool("system_set",dict(system=rich,revision=current["revision"]))
    before=tool("system_get");saved=(DATA/"system.json").read_bytes()
    for args in [{},dict(revision=before["revision"]+1),dict(revision=before["revision"],extra=1),dict(revision=1.5)]:
        tool("project_new",args,error=True)
    assert tool("system_get")==before and (DATA/"system.json").read_bytes()==saved
    pending=DATA/"system.pending";pending.mkdir()
    try: tool("project_new",dict(revision=before["revision"]),error=True)
    finally: pending.rmdir()
    assert tool("system_get")==before and (DATA/"system.json").read_bytes()==saved
    created=tool("project_new",dict(revision=before["revision"]))
    assert created["revision"]==before["revision"]+1
    new=tool("system_get");s=new["system"]
    assert s["name"]=="Untitled system" and s["note"]=="" and s["custom_glasses"]==[]
    assert len(s["surfaces"])==1 and s["surfaces"][0]["gap_solve"]==dict(kind="fixed")
    assert s["surfaces"][0]["radius"]==0 and s["surfaces"][0]["thickness"]==0 and not s["surfaces"][0]["clear_aperture"]
    assert s["stop_index"]==0 and s["object_distance"] is None and s["object_material"]=="AIR"
    assert s["object_label"]=="Object" and s["image_label"]=="Image"
    assert len(s["fields"])==len(s["wavelengths"])==1 and s["wavelengths"][0]["value"]==587.5618
    assert s["fields"][0]["vignetting"]==[0,0,0,0]
    assert tool("capabilities")["scope"]["project_creation"]
    tool("project_new",dict(revision=before["revision"]),error=True)
    export=tool("project_export",dict(format="hissabeth"))
    assert export["revision"]==new["revision"] and export["name"]==s["name"] and export["format"]=="hissabeth"
    native=export["text"]
    assert json.loads(native)==s
    ray=tool("trace_ray",dict(x=3,y=4,field_index=0))
    assert ray["points"][-1]==[3,4,0]
    first_order=tool("paraxial")
    assert first_order["revision"]==new["revision"]
    assert first_order["matrix"]==[1,0,0,1] and first_order["image_na"]==0
    assert first_order["entrance_pupil_radius"]==first_order["exit_pupil_radius"]==5
    for key in ["efl","bfl","front_focal_distance","image_distance","f_number","magnification"]:
        assert first_order[key] is None, (key,first_order[key])
    for name in ["longitudinal_aberration","chromatic_focus","spherochromatism"]:
        tool(name,error=True)
    tool("autofocus",dict(revision=new["revision"]),error=True)
    assert tool("system_get")==new
    engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink(missing_ok=True);engine=start_engine()
    assert tool("system_get")["system"]==s
    assert tool("paraxial")["image_distance"] is None
    print("PASS: real MCP new-project schemas/stale revisions, failed-write atomicity, clean blank specification, native export, unpowered tracing, rejected focus and same-relay restart")


def surface_edit():
    global engine
    current=tool("system_get")
    tool("system_set",dict(system=current["system"],revision=current["revision"]))
    current=tool("system_get"); before=(DATA/"system.json").read_bytes()
    draft=json.loads(json.dumps(current["system"]))
    draft["name"]="Staged topology only"
    draft["fields"][0]["weight"]=0
    draft["wavelengths"].append(dict(draft["wavelengths"][0],weight=0))
    draft["surfaces"][0]["gap_solve"]=dict(kind="pickup",source=1,scale=.1,offset=2)
    args=dict(system=draft,operation=dict(kind="insert",before=1,count=2))
    result=tool("surface_edit",args)
    assert result["old_to_new"]==[0,3] and result["inserted"]==[1,2]
    assert not result["stop_reassigned"] and "revision" not in result
    staged=result["system"]
    assert staged["name"]==draft["name"] and staged["fields"]==draft["fields"] and staged["wavelengths"]==draft["wavelengths"]
    assert staged["surfaces"][0]["gap_solve"]["source"]==3
    assert staged["surfaces"][0]["thickness"]==draft["surfaces"][0]["thickness"]
    assert all(r["interaction"]=="dummy" and r["thickness"]==0 and not r["clear_aperture"] for r in staged["surfaces"][1:3])
    tool("surface_edit",dict(system=draft,operation=dict(kind="remove",first=1,count=1)),error=True)
    for op in [dict(kind="insert",before=0,count=0),dict(kind="insert",before=0,count=1.5),dict(kind="insert",before=0,count=1,extra=True),dict(kind="remove",first=0,count=2)]:
        tool("surface_edit",dict(system=draft,operation=op),error=True)
    bad=json.loads(json.dumps(draft));bad["surfaces"][0]["material"]="UnfinishedGlass"
    assert tool("surface_edit",dict(system=bad,operation=dict(kind="insert",before=0,count=1)))["system"]["surfaces"][1]["material"]=="UnfinishedGlass"
    assert tool("system_get")==current and (DATA/"system.json").read_bytes()==before
    tool("system_set",dict(system=staged,revision=current["revision"]))
    applied=tool("system_get")
    assert applied["revision"]==current["revision"]+1
    assert abs(applied["system"]["surfaces"][0]["thickness"]-(draft["surfaces"][1]["thickness"]*.1+2))<1e-9
    tool("surface_edit",dict(system=staged,operation=dict(kind="remove",first=0,count=3)))
    engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink(missing_ok=True);engine=start_engine()
    recovered=tool("system_get")
    assert recovered["system"]==applied["system"]
    assert tool("capabilities")["scope"]["surface_row_editing"]
    print("PASS: real MCP draft topology, strict schemas, shared reference guards, no implicit save/solve, explicit constrained Apply and same-relay restart")


def prescription_report():
    global engine
    import copy
    snapshot=tool("system_get")
    tool("system_set",dict(system=snapshot["system"],revision=snapshot["revision"]))
    initial=tool("system_get");revision=initial["revision"]
    disk=(DATA/"system.json").read_bytes()
    all_sections=["general","specification","first_order","lens_data","surface_data"]
    args=dict(sections=list(reversed(all_sections)),wavelength=587.5618,precision=6)
    result=tool("prescription_report",args)
    assert result["revision"]==revision and [s["id"] for s in result["sections"]]==all_sections
    assert tool("capabilities")["scope"]["assembled_prescription_report"]
    assert tool("system_get")==initial and (DATA/"system.json").read_bytes()==disk
    for invalid in [{},dict(args,precision=16),dict(args,precision=1.5),dict(args,wavelength=99),dict(args,sections=["general","general"]),dict(args,sections=["invalid"]),dict(args,revision=revision)]:
        tool("prescription_report",invalid,error=True)
    empty=tool("prescription_report",dict(args,sections=[]))
    assert empty["sections"]==[] and empty["diagnostics"]==[] and "Sections: none" in empty["text"]
    system=copy.deepcopy(initial["system"])
    system["name"]="Report\ttitle\nnext";system["note"]="First line\nSecond line"
    system["wavelengths"].append(dict(system["wavelengths"][0],weight=0))
    system["image_surface"].update(even=[1e-20,0],odd=[1e-8,0])
    revision=tool("system_set",dict(system=system,revision=revision))["revision"]
    result=tool("prescription_report",args)
    def table(section,name):
        return next(t for s in result["sections"] if s["id"]==section for t in s["tables"] if t["id"]==name)
    assert "Title: Report\\ttitle\\nnext" in result["text"] and "First line\nSecond line" in result["text"]
    assert len(table("specification","wavelengths")["rows"])==4
    profiles={r["id"]:r["cells"][1] for r in table("surface_data","image")["rows"]}
    assert profiles["even_4"]["value"]==1e-20 and profiles["even_4"]["text"]=="1.000000e-20"
    assert profiles["odd_4"]["value"]==0
    for section in result["sections"]:
        for t in section["tables"]:
            assert all(len(row["cells"])==len(t["columns"]) for row in t["rows"])
    partial=tool("prescription_report",dict(args,wavelength=100))
    assert partial["sections"] and partial["diagnostics"]
    engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink();engine=start_engine()
    recovered=tool("prescription_report",args)
    assert recovered["revision"]!=revision and recovered["sections"]==result["sections"]
    print("PASS: real prescription-report schemas, canonical sections and text, exact small/zero coefficients, duplicate/zero-weight spectra, localized spectral diagnostics, read-only saved state and same-relay restart")


def field_curvature():
    global engine
    import copy
    initial=tool("system_get");revision=initial["revision"]
    result=tool("field_curvature",dict(samples=17))
    assert result["revision"]==revision and result["field_end"]==[0,5]
    assert len(result["curves"])==3 and all(len(c["points"])==17 for c in result["curves"])
    assert tool("capabilities")["scope"]["continuous_field_curvature"]
    for args in [{},dict(samples=1),dict(samples=258),dict(samples=17,field_end=[1]),dict(samples=17,wavelength=550)]:
        tool("field_curvature",args,error=True)
    assert tool("system_get")["revision"]==revision
    system=copy.deepcopy(initial["system"])
    duplicate=copy.deepcopy(system["wavelengths"][0]);duplicate.update(weight=0,color=[.8,.2,.6,.25])
    system["wavelengths"].append(duplicate)
    system["fields"]=[dict(system["fields"][0],x=3,y=4)]
    revision=tool("system_set",dict(system=system,revision=revision))["revision"]
    args=dict(samples=17,field_end=[3,4])
    result=tool("field_curvature",args)
    assert len(result["curves"])==4
    assert result["curves"][3]["weight"]==0 and result["curves"][3]["color"]==duplicate["color"]
    assert result["curves"][0]["points"]==result["curves"][3]["points"]
    for curve in result["curves"]:
        for i in [0,8,16]:
            point=curve["points"][i]
            assert point["field"]==[3*i/16,4*i/16]
            direct=tool("astigmatism",dict(field_x=point["field"][0],field=point["field"][1],wavelength=curve["wavelength"]))
            for branch in ["sagittal","tangential"]:
                assert point[branch]["error"] is None
                assert abs(point[branch]["shift"]-direct[branch+"_shift"])<1e-10
    system["image_surface"].update(clear_aperture=True,aperture=.2)
    revision=tool("system_set",dict(system=system,revision=revision))["revision"]
    clipped=tool("field_curvature",args)
    for curve in clipped["curves"]:
        assert len(curve["points"])==17
        assert curve["points"][0]["sagittal"]["error"] is None
        assert any(p["sagittal"]["error"] for p in curve["points"][1:])
        for point in curve["points"]:
            if point["sagittal"]["error"]:
                assert point["sagittal"]["shift"] is None
                assert point["sagittal"]["error"]["endpoint"]=="image"
    engine.terminate();engine.wait(timeout=5);(DATA/"endpoint.json").unlink();engine=start_engine()
    recovered=tool("field_curvature",args)
    assert recovered["curves"]==clipped["curves"] and recovered["revision"]!=revision
    system["image_surface"]["clear_aperture"]=False
    system["surfaces"]=[dict(system["surfaces"][0],radius=0,conic=0,even=[],odd=[],interaction="dummy",thickness=10)]
    system["stop_index"]=0
    tool("system_set",dict(system=system,revision=tool("system_get")["revision"]))
    empty=tool("field_curvature",dict(samples=2,field_end=[0,0]))
    assert empty["max_field"]==0
    for curve in empty["curves"]:
        for point in curve["points"]:
            for branch in ["sagittal","tangential"]:
                assert point[branch]["shift"] is None and point[branch]["error"]["kind"]=="at_infinity"
    print("PASS: real field-curvature schemas, all spectral entries, skew sampling, shared analytic astigmatism values, retained chief failures and all-infinite/zero-field diagnostics, read-only revisions and same-relay recovery")


def ray_inspection():
    global engine
    snapshot = tool("system_get")
    revision = snapshot["revision"]
    w = snapshot["system"]["wavelengths"][snapshot["system"]["reference_wavelength"]]["value"]
    assert {t["name"] for t in tools} >= {"trace_inspect", "paraxial_trace"}
    capabilities = tool("capabilities")["scope"]
    assert capabilities["ray_inspection"] and capabilities["paraxial_ray_tables"]
    launch = dict(kind="pupil", pupil=[0.1, 0.2], field_index=0)
    radius = tool("paraxial")["entrance_pupil_radius"]
    exact = tool("trace_ray", dict(x=0.1*radius, y=0.2*radius, field_index=0, wavelength=w))
    result = tool("trace_inspect", dict(launch=launch, wavelength=w))
    assert result["failure"] is None and result["revision"] == revision
    assert result["ray"] == {k:v for k,v in exact.items() if k != "revision"}
    table = tool("paraxial_trace", dict(field_index=1, wavelength=w))
    assert len(table["axis"]) == len(snapshot["system"]["surfaces"])+1
    assert table["chief"][0]["endpoint"] == "surface" and table["chief"][-1]["endpoint"] == "image"
    for bad in [dict(launch=launch), dict(launch={**launch,"pupil":[1]},wavelength=w),
                dict(launch={**launch,"point":[0,0]},wavelength=w),
                dict(launch={**launch,"field_index":63},wavelength=w),
                dict(launch=dict(kind="object",point=[0,0],tangent=[0,0]),wavelength=w)]:
        tool("trace_inspect",bad,error=True)
    assert tool("system_get")["revision"] == revision
    system = snapshot["system"]
    system["object_distance"] = 100
    system["object_surface"]["radius"] = 20
    system["pupil"]["value"] = 1
    system["surfaces"] = [dict(system["surfaces"][0],radius=0,thickness=10,material="AIR",aperture=20,clear_aperture=True)]
    system["stop_index"] = 0
    system["fields"] = [dict(system["fields"][0],x=0,y=0)]
    revision = tool("system_set",dict(system=system,revision=revision))["revision"]
    args = dict(launch=dict(kind="object",point=[1,2],tangent=[0.1,-0.05]),wavelength=w)
    result = tool("trace_inspect",args)
    ray = result["ray"]
    z = -100+20-math.sqrt(395)
    assert result["failure"] is None and abs(ray["object_point"][2]-z)<1e-12
    assert ray["points"][0] == ray["object_point"] and ray["entrance_opl"] == 0
    expected_x = 1+(10-z)*0.1
    expected_y = 2+(10-z)*(-0.05)
    assert abs(ray["image_hit"]["point"][0]-expected_x)<1e-10
    assert abs(ray["image_hit"]["point"][1]-expected_y)<1e-10
    assert abs(ray["opl"]-(10-z)*math.sqrt(1.0125))<1e-10
    table = tool("paraxial_trace",dict(field_index=0,wavelength=w))
    assert [r["endpoint"] for r in table["axis"]] == ["object","surface","image"]
    assert table["axis"][0]["point"] == [0,0,-100]
    system["image_surface"].update(clear_aperture=True,aperture=0.1)
    revision = tool("system_set",dict(system=system,revision=revision))["revision"]
    failed = tool("trace_inspect",args)
    assert failed["failure"]["endpoint"] == "image" and failed["failure"]["kind"] == "blocked"
    assert len(failed["ray"]["segments"]) == 1 and failed["ray"]["image_hit"] is None
    assert tool("system_get")["revision"] == revision
    engine.terminate()
    engine.wait(timeout=5)
    (DATA / "endpoint.json").unlink()
    engine = start_engine()
    recovered = tool("trace_inspect",args)
    assert recovered["ray"] == failed["ray"] and recovered["failure"] == failed["failure"]
    assert recovered["revision"] != revision
    print("PASS: typed inspection schemas; normalized pupil identity, real curved source XY/slopes/OPL, paraxial vertex tables, retained physical failure, read-only revisions and same-relay recovery")


def persistent_solves():
    global engine
    initial = tool("system_get")
    definition = dict(kind="pickup", source=0, scale=2, offset=1)
    result = tool("solve", dict(gap=1, definition=definition, revision=initial["revision"]))
    assert result["solve"]["definition"] == definition
    assert result["solve"]["thickness"] == 11
    current = tool("system_get")
    current["system"]["surfaces"][0]["thickness"] = 7
    current["system"]["surfaces"][1]["thickness"] = 999
    preview = tool("system_validate", dict(system=current["system"]))
    assert preview["system"]["surfaces"][1]["thickness"] == 15
    assert tool("system_get")["system"]["surfaces"][1]["thickness"] == 11
    tool("system_set", dict(system=current["system"], revision=current["revision"]))
    current = tool("system_get")
    assert current["system"]["surfaces"][1]["thickness"] == 15
    saved = (DATA / "system.json").read_bytes()
    for name, arguments in [
        ("autofocus", dict(revision=current["revision"])),
        ("solve", dict(gap=0, definition=dict(kind="pickup", source=1, scale=0.5, offset=0), revision=current["revision"])),
        ("solve", dict(gap=1, definition=dict(kind="fixed", value=0), revision=current["revision"])),
        ("solve_recalculate", dict(revision=current["revision"]-1)),
    ]:
        tool(name, arguments, error=True)
        assert tool("system_get") == current
        assert (DATA / "system.json").read_bytes() == saved
    native = tool("project_export", dict(format="hissabeth"))["text"]
    imported = json.loads(native)
    assert imported["surfaces"][1]["gap_solve"] == definition
    imported["surfaces"][0]["thickness"] = 8
    imported["surfaces"][1]["thickness"] = -100
    tool("project_import", dict(format="hissabeth", text=json.dumps(imported), revision=current["revision"]))
    current = tool("system_get")
    assert current["system"]["surfaces"][1]["thickness"] == 17
    tool("project_export", dict(format="geopter"), error=True)
    engine.terminate()
    engine.wait(timeout=5)
    (DATA / "endpoint.json").unlink()
    # Recovery must reevaluate stored definitions to replace stale derived thicknesses.
    imported["surfaces"][0]["thickness"] = 9
    imported["surfaces"][1]["thickness"] = 1234
    (DATA / "system.json").write_text(json.dumps(imported))
    engine = start_engine()
    recovered = tool("system_get")
    assert recovered["system"]["surfaces"][1]["thickness"] == 19
    assert json.loads((DATA / "system.json").read_text())["surfaces"][1]["thickness"] == 19
    tool("solve_recalculate", dict(revision=current["revision"]), error=True)
    result = tool("solve_recalculate", dict(revision=recovered["revision"]))
    assert result["solves"] == [dict(gap=1, thickness=19, definition=definition)]
    result = tool("solve", dict(gap=1, definition=dict(kind="fixed"), revision=result["revision"]))
    current = tool("system_get")
    assert current["system"]["surfaces"][1]["gap_solve"] == dict(kind="fixed")
    current["system"]["surfaces"][0]["thickness"] = 12
    tool("system_set", dict(system=current["system"], revision=current["revision"]))
    assert tool("system_get")["system"]["surfaces"][1]["thickness"] == 19
    print("PASS: persistent solve MCP definitions, edit/preview/import reapplication, singular/stale/invalid rejection without mutation, native export, same-relay recovery of stale derived values, recalculation and removal")


def apertures():
    global engine
    current = tool("system_get")
    system = current["system"]
    import copy
    plane = copy.deepcopy(system["surfaces"][0])
    plane.update(radius=0, thickness=10, material="AIR", aperture=1, clear_aperture=False, conic=0, even=[], odd=[], semi_diameter=999)
    second = copy.deepcopy(plane)
    second.update(aperture=2, clear_aperture=True)
    system.update(surfaces=[plane, second], fields=[dict(x=0, y=math.degrees(math.atan(.1)), weight=1, vignetting=[0]*4,color=[.2,.5,.8,1])], pupil=dict(kind="entrance_pupil_diameter", value=10), object_distance=None, stop_index=0)
    tool("system_set",dict(system=system,revision=current["revision"]))
    current=tool("system_get")
    assert abs(current["system"]["surfaces"][1]["semi_diameter"]-6)<1e-8
    report=tool("aperture_report")
    assert report["revision"]==current["revision"] and report["failed_rays"]==0
    update=tool("vignetting_update",dict(revision=current["revision"]))
    expected=[.8,.4,1-math.sqrt(3)/5,1-math.sqrt(3)/5]
    assert all(abs(a-b)<2e-9 for a,b in zip(update["fields"][0]["vignetting"],expected))
    current=tool("system_get")
    assert current["system"]["surfaces"][1]["aperture"]==2
    assert current["system"]["surfaces"][1]["semi_diameter"]<6
    native=json.loads(tool("project_export",dict(format="hissabeth"))["text"])
    assert native["surfaces"][1]["semi_diameter"]==current["system"]["surfaces"][1]["semi_diameter"]
    current["system"]["surfaces"][1]["aperture"]=.5
    tool("system_set",dict(system=current["system"],revision=current["revision"]))
    failed=tool("system_get")
    saved=(DATA/"system.json").read_bytes()
    tool("vignetting_update",dict(revision=failed["revision"]),error=True)
    assert tool("system_get")==failed and (DATA/"system.json").read_bytes()==saved
    for surface in failed["system"]["surfaces"]: surface["clear_aperture"]=False
    tool("system_set",dict(system=failed["system"],revision=failed["revision"]))
    current=tool("system_get")
    tool("vignetting_update",dict(revision=current["revision"]))
    current=tool("system_get")
    assert current["system"]["fields"][0]["vignetting"]==[0]*4
    engine.terminate();engine.wait(timeout=5)
    stored=json.loads((DATA/"system.json").read_text())
    stored["surfaces"][1]["semi_diameter"]=999
    (DATA/"system.json").write_text(json.dumps(stored))
    (DATA/"endpoint.json").unlink(missing_ok=True)
    engine=start_engine()
    recovered=tool("system_get")
    assert abs(recovered["system"]["surfaces"][1]["semi_diameter"]-6)<1e-8
    assert json.loads((DATA/"system.json").read_text())==recovered["system"]
    print("PASS: aperture/vignetting MCP revision, sizing, factors, native persistence, atomic rejection, clear and same-relay corrected-size restart recovery")


def spectrum_sampling():
    import copy
    initial=tool("system_get")
    system=copy.deepcopy(initial["system"])
    plane=copy.deepcopy(system["surfaces"][0])
    plane.update(radius=0,thickness=10,material="AIR",aperture=2,clear_aperture=True,semi_diameter=0,conic=0,even=[],odd=[])
    system.update(surfaces=[plane],fields=[dict(x=0,y=0,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])],pupil=dict(kind="entrance_pupil_diameter",value=2),stop_index=0,object_distance=None)
    tool("system_set",dict(system=system,revision=initial["revision"]))
    current=tool("system_get")
    sampling=dict(kind="hexapolar",rings=2)
    spot=tool("spot_diagram",dict(field_index=0,sampling=sampling))
    assert spot["revision"]==current["revision"] and spot["sampled_rays"]==19
    assert abs(spot["rms_radius"]**2-.5625)<1e-12
    assert abs(sum(spot["weights"])-1)<1e-12
    energy=tool("encircled_energy",dict(field_index=0,sampling=sampling,samples=5))
    assert abs(energy["points"][3]["fraction"]-.5625)<1e-12
    assert energy["revision"]==current["revision"]
    before=tool("system_get")
    for name,args in [
        ("spot_diagram",dict(grid=17,sampling=sampling)),
        ("spot_diagram",dict(sampling=dict(kind="grid",count=17,rings=2))),
        ("geometric_mtf",dict(sampling=sampling,wavelength=500,spectrum=dict(kind="polychromatic"))),
        ("diffraction_mtf",dict(grid=16,spectrum=dict(kind="polychromatic",wavelength=500))),
    ]: tool(name,args,error=True)
    assert tool("system_get")==before
    system=copy.deepcopy(initial["system"])
    system.update(stop_index=1,pupil=dict(kind="entrance_pupil_diameter",value=.00001),fields=[dict(x=0,y=5,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])],wavelengths=[dict(value=486.1327,weight=1,color=[.8,.7,0,1]),dict(value=656.2725,weight=1,color=[.8,.7,0,1]),dict(value=435.834,weight=0,color=[.8,.7,0,1])],reference_wavelength=0)
    tool("system_set",dict(system=system,revision=before["revision"]))
    current=tool("system_get")
    a=tool("trace_ray",dict(field_index=0,x=0,y=0,wavelength=486.1327))["points"][-1][1]
    b=tool("trace_ray",dict(field_index=0,x=0,y=0,wavelength=656.2725))["points"][-1][1]
    maximum=1/(2*abs(a-b))
    args=dict(field_index=0,sampling=sampling,spectrum=dict(kind="polychromatic"),axis="y",max_frequency=maximum)
    mtf=tool("geometric_mtf",args)
    assert mtf["revision"]==current["revision"] and len(mtf["components"])==2
    assert mtf["mtf"][-1]<1e-4
    for wave in (486.1327,656.2725):
        mono=tool("geometric_mtf",dict(args,spectrum=dict(kind="monochromatic",wavelength=wave)))
        assert mono["mtf"][-1]>.9999
    polyspot=tool("polychromatic_spot",dict(field_index=0,sampling=sampling))
    assert len(polyspot["failures"])==2 and polyspot["sampled_rays"]==38
    assert abs(sum(polyspot["weights"])-polyspot["transmitted_weight"])<1e-12
    ray_a=tool("trace_ray",dict(field_index=0,x=0,y=0,wavelength=486.1327))
    ray_b=tool("trace_ray",dict(field_index=0,x=0,y=0,wavelength=656.2725))
    current["system"]["surfaces"][0]["aperture"]=(abs(ray_a["segments"][0]["point"][1])+abs(ray_b["segments"][0]["point"][1]))/2
    tool("system_set",dict(system=current["system"],revision=current["revision"]))
    current=tool("system_get");saved=(DATA/"system.json").read_bytes()
    error=tool("diffraction_mtf",dict(field_index=0,grid=16,spectrum=dict(kind="polychromatic")),error=True)
    assert "blocked" in error["content"][0]["text"]
    geometric=tool("geometric_mtf",args)
    assert sum(c["transmitted_samples"]==0 for c in geometric["components"])==1
    assert tool("system_get")==current and (DATA/"system.json").read_bytes()==saved
    current["system"]["surfaces"][0]["aperture"]=12.5
    current["system"]["pupil"]["value"]=4
    current["system"]["fields"][0].update(x=1.8,y=2.4)
    tool("system_set",dict(system=current["system"],revision=current["revision"]))
    current=tool("system_get")
    diffraction=tool("diffraction_mtf",dict(field_index=0,grid=16,spectrum=dict(kind="polychromatic")))
    assert diffraction["revision"]==current["revision"] and len(diffraction["components"])==2
    assert diffraction["max_frequency"]==diffraction["sampling_limit"]
    for axis in ("sagittal","tangential"):
        assert abs(diffraction[axis][0]-1)<1e-12
        for magnitude,re,im in zip(diffraction[axis],diffraction[axis+"_otf"]["real"],diffraction[axis+"_otf"]["imaginary"]):
            assert abs(magnitude-math.hypot(re,im))<1e-12
    tool("diffraction_mtf",dict(grid=16,spectrum=dict(kind="polychromatic"),max_frequency=2*diffraction["sampling_limit"]),error=True)
    current=tool("system_get");saved=(DATA/"system.json").read_bytes()
    for name,args in [("geometric_mtf",dict(sampling=sampling,spectrum=dict(kind="monochromatic",wavelength=3000))),("diffraction_mtf",dict(grid=16,spectrum=dict(kind="monochromatic",wavelength=3000)))]:
        error=tool(name,args,error=True)
        assert "3000" in error["content"][0]["text"],error
    assert tool("system_get")==current and (DATA/"system.json").read_bytes()==saved
    # The upstream prescriptions import with the reference entrance-pupil mapping.
    source=ROOT.parent/"reference/upstream/example/book"
    for filename in ("sasian_triplet.json","kingslake_doublet.json","sasian_triplet_glass.json"):
        current=tool("system_get")
        tool("project_import",dict(format="geopter",text=(source/filename).read_text(),revision=current["revision"]))
        assert tool("system_get")["system"]["pupil_mapping"]=="reference_entrance_pupil"
        native=json.loads(tool("project_export",dict(format="hissabeth"))["text"])
        assert native["pupil_mapping"]=="reference_entrance_pupil"
    print("PASS: weighted hexapolar spot/energy, strict spectral/sampling arguments, real lateral-color OTF cancellation, diffraction physical frequencies/complex components, skipped zero weights, positive-weight failure without mutation, engine analysis revisions")


def mirrors():
    import copy
    global engine
    initial=tool("system_get");system=copy.deepcopy(initial["system"])
    surface=copy.deepcopy(system["surfaces"][0])
    surface.update(radius=-100,thickness=-50,material="N-BK7_SCHOTT",interaction="reflect",aperture=10,clear_aperture=True,conic=-1,even=[],odd=[],gap_solve=dict(kind="fixed"))
    system.update(surfaces=[surface],fields=[dict(x=0,y=0,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])],stop_index=0,
                  object_distance=None,object_material="AIR",temperature=25,pupil=dict(kind="entrance_pupil_diameter",value=10))
    tool("system_set",dict(system=system,revision=initial["revision"]))
    capability=tool("capabilities")["scope"]
    assert capability["centered_reflection"] and not capability["folded_coordinates"]
    paraxial=tool("paraxial")
    assert paraxial["efl"]==50 and paraxial["bfl"]==-50 and paraxial["image_distance"]==-50
    assert paraxial["image_index"]==1 and paraxial["image_direction"]==-1
    assert "direction_sign" in paraxial["matrix_basis"]
    ray=tool("trace_ray",dict(field_index=0,x=3,y=4))
    assert max(abs(a-b) for a,b in zip(ray["points"][-1],[0,0,-50]))<1e-10
    assert abs(ray["opl"]+ray["entrance_opl"]-50)<1e-10
    assert ray["segments"][0]["n_before"]==ray["segments"][0]["n_after"]==1
    assert ray["direction"][2]<0
    cases=[("ray_fan",dict(samples=9)),("opd_fan",dict(samples=9)),("chromatic_focus",dict(lower=480,upper=650,samples=5)),
           ("longitudinal_aberration",dict(samples=9)),("spherochromatism",dict(samples=9)),("astigmatism",{}),("distortion",{}),
           ("encircled_energy",dict(sampling=dict(kind="hexapolar",rings=4),samples=9)),("wavefront",dict(grid=16)),
           ("diffraction_psf",dict(grid=16)),("diffraction_mtf",dict(grid=16,spectrum=dict(kind="polychromatic"))),
           ("geometric_mtf",dict(sampling=dict(kind="hexapolar",rings=4),spectrum=dict(kind="polychromatic")))]
    results={name:tool(name,args if name=="chromatic_focus" else dict(field_index=0,**args)) for name,args in cases}
    psf=results["diffraction_psf"]
    assert abs(sum(psf["intensity"])-1)<1e-12 and abs(psf["strehl"]-1)<1e-10
    assert abs(psf["pixel_size"]-system["wavelengths"][system["reference_wavelength"]]["value"]*1e-6*2.5)<1e-14
    assert all(abs(v)<1e-7 for v in results["wavefront"]["opd"] if v is not None)
    mono=tool("spot_diagram",dict(field_index=0,sampling=dict(kind="grid",count=17)))
    poly=tool("polychromatic_spot",dict(field_index=0,sampling=dict(kind="hexapolar",rings=4)))
    assert mono["blocked"]==0 and mono["rms_radius"]<1e-10 and poly["rms_radius"]<1e-10
    state=tool("system_get");stored=(DATA/"system.json").read_bytes()
    tool("project_export",dict(format="geopter"),error=True)
    broken=copy.deepcopy(state["system"]);broken["pupil"]=dict(kind="image_na",value=1.2)
    tool("system_set",dict(system=broken,revision=state["revision"]),error=True)
    assert tool("system_get")==state and (DATA/"system.json").read_bytes()==stored
    native=tool("project_export",dict(format="hissabeth"))["text"]
    tool("project_import",dict(format="hissabeth",text=native,revision=state["revision"]))
    assert tool("system_get")["system"]==state["system"]
    tool("solve",dict(gap=0,definition=dict(kind="edge_thickness",height=3,value=-60),revision=tool("system_get")["revision"]))
    saved=tool("system_get")
    assert abs(saved["system"]["surfaces"][0]["thickness"]-(-60-9/200))<1e-10
    saved["system"]["surfaces"][0]["radius"]=-120
    tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
    saved=tool("system_get");expected=-60-9/240
    assert abs(saved["system"]["surfaces"][0]["thickness"]-expected)<1e-10
    engine.terminate();engine.wait(timeout=5)
    stale=copy.deepcopy(saved["system"]);stale["surfaces"][0]["thickness"]=999
    stale["surfaces"][0]["semi_diameter"]=stale["image_surface"]["semi_diameter"]=999
    (DATA/"system.json").write_text(json.dumps(stale));(DATA/"endpoint.json").unlink(missing_ok=True)
    engine=start_engine();recovered=tool("system_get")["system"]
    assert abs(recovered["surfaces"][0]["thickness"]-expected)<1e-10
    assert recovered["surfaces"][0]["interaction"]=="reflect"
    assert json.loads((DATA/"system.json").read_text())==recovered
    # A plane mirror has finite virtual imaging even though all focal lengths are infinite.
    plane=copy.deepcopy(system);plane.update(object_distance=300,field_type="object_height",pupil=dict(kind="entrance_pupil_diameter",value=2))
    plane["fields"][0].update(x=1.2,y=-2)
    plane["surfaces"][0].update(radius=0,conic=0,thickness=10)
    tool("system_set",dict(system=plane,revision=tool("system_get")["revision"]))
    p=tool("paraxial")
    assert all(p[key] is None for key in ("efl","bfl","front_focal_distance"))
    assert abs(p["image_distance"]-300)<1e-10 and p["magnification"]==1 and p["image_direction"]==-1
    tool("autofocus",dict(revision=tool("system_get")["revision"]))
    ray=tool("trace_ray",dict(field_index=0,x=.4,y=-.3))
    assert max(abs(a-b) for a,b in zip(ray["points"][-1],[1.2,-2,300]))<1e-10
    assert abs(ray["opl"]+ray["entrance_opl"])<1e-10
    wf=tool("wavefront",dict(field_index=0,grid=16))
    assert all(abs(v)<1e-7 for v in wf["opd"] if v is not None)
    # Positive parity after two mirrors; the element coordinates stay globally signed.
    telescope=copy.deepcopy(system);a=telescope["surfaces"][0];a.update(radius=-200,conic=0,thickness=-80)
    b=copy.deepcopy(a);b.update(radius=-50,thickness=100);telescope["surfaces"].append(b)
    tool("system_set",dict(system=telescope,revision=tool("system_get")["revision"]))
    p=tool("paraxial")
    assert p["image_direction"]==1 and p["image_index"]==1
    assert abs(p["efl"]-500)<1e-9 and abs(p["bfl"]-100)<1e-9
    ray=tool("trace_ray",dict(field_index=0,y=.0001))
    assert abs(ray["points"][-1][1])<1e-10 and abs(ray["points"][-1][2]-20)<1e-10
    print("PASS: real MCP centered mirrors, signed Gaussian matrix/physical indices, exact parabolic focus and OPL, all analysis families and mono/poly spectra, signed edge solve/profile reapplication, strict failure atomicity/native retention/same-relay recovery, plane-mirror finite virtual imaging/autofocus, and two-mirror telescope")


def endpoints():
    import copy
    global engine
    initial=tool("system_get");system=copy.deepcopy(initial["system"])
    plane=copy.deepcopy(system["surfaces"][0])
    plane.update(radius=0,thickness=20,material="AIR",aperture=10,clear_aperture=True,conic=0,even=[],odd=[],gap_solve=dict(kind="fixed"))
    system.update(surfaces=[plane],object_distance=100,object_material="AIR",object_label="Curved emitter",image_label="Curved detector",
                  field_type="object_height",fields=[dict(x=1,y=2,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])],stop_index=0,
                  pupil=dict(kind="entrance_pupil_diameter",value=2),pupil_mapping="reference_entrance_pupil",
                  wavelengths=[dict(value=587.562,weight=1,color=[.7,.8,0,1])],reference_wavelength=0,temperature=25)
    system["object_surface"].update(radius=50,clear_aperture=True,aperture=3)
    system["image_surface"].update(radius=40)
    tool("system_set",dict(system=system,revision=initial["revision"]))
    for gap in (20,-20):
        saved=tool("system_get");saved["system"]["surfaces"][0]["thickness"]=gap
        tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
        ray=tool("trace_ray",dict(field_index=0,x=.25,y=-.1))
        source=[1,2,-50-math.sqrt(2495)]
        assert max(abs(a-b) for a,b in zip(ray["object_point"],source))<1e-11
        center=[0,0,gap+40];first=ray["segments"][0]["point"];direction=ray["direction"]
        delta=[a-b for a,b in zip(first,center)];b=sum(a*d for a,d in zip(delta,direction))
        root=math.sqrt(b*b-(sum(a*a for a in delta)-1600))
        t=min([-b-root,-b+root],key=lambda t:abs(t-gap/direction[2]))
        expected=[a+t*d for a,d in zip(first,direction)]
        assert max(abs(a-b) for a,b in zip(ray["points"][-1],expected))<1e-10
        assert abs(ray["opl"]+ray["entrance_opl"]-(math.dist(source,first)+t))<1e-10
        assert ray["image_hit"]["point"]==ray["points"][-1]
        assert ray["image_hit"]["n_before"]==ray["image_hit"]["n_after"]==1
    for odd_object in (False,True):
        saved=tool("system_get");saved["system"]["surfaces"][0]["thickness"]=20
        for key in ("object_surface","image_surface"):
            saved["system"][key].update(conic=-1,even=[],odd=[])
        saved["system"]["object_surface"]["odd" if odd_object else "even"]=[.000001]
        saved["system"]["image_surface"]["even" if odd_object else "odd"]=[.0001]
        tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
        expected=tool("system_get")["system"]
        for format in ("hissabeth","geopter"):
            text=tool("project_export",dict(format=format))["text"]
            tool("project_import",dict(format=format,text=text,revision=tool("system_get")["revision"]))
            actual=tool("system_get")["system"]
            for key in ("object_surface","image_surface","object_label","image_label"):
                shape=copy.deepcopy(expected[key])
                if format=="geopter" and isinstance(shape,dict):
                    for terms in ("even","odd"):
                        if shape[terms]: shape[terms]+=[0]*(10-len(shape[terms]))
                assert actual[key]==shape,(format,key)
    report=tool("aperture_report")
    assert abs(report["object"]["semi_diameter"]-math.sqrt(5))<1e-12
    assert report["object"]["reached_rays"]==report["image"]["reached_rays"]==5
    saved=tool("system_get");saved["system"]["object_surface"]["aperture"]=1
    tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
    state=tool("system_get");stored=(DATA/"system.json").read_bytes()
    error=tool("trace_ray",dict(field_index=0),error=True)
    assert "object aperture" in error["content"][0]["text"].lower()
    tool("vignetting_update",dict(revision=state["revision"]),error=True)
    assert tool("system_get")==state and (DATA/"system.json").read_bytes()==stored
    saved=tool("system_get");system=saved["system"]
    system.update(object_distance=None,field_type="angle",fields=[dict(x=0,y=0,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])])
    system["object_surface"]=copy.deepcopy(initial["system"]["object_surface"])
    system["image_surface"].update(radius=10,conic=0,even=[],odd=[],clear_aperture=True,aperture=.55)
    system["surfaces"][0]["clear_aperture"]=False
    tool("system_set",dict(system=system,revision=saved["revision"]))
    args=dict(field_index=0,sampling=dict(kind="grid",count=9))
    spot=tool("spot_diagram",args)
    assert spot["sampled_rays"]==49 and spot["blocked"]==36 and len(spot["points"])==13
    assert abs(spot["transmitted_weight"]-13/49)<1e-12
    report=tool("aperture_report");assert report["image"]["semi_diameter"]==1 and report["image"]["reached_rays"]==5
    update=tool("vignetting_update",dict(revision=tool("system_get")["revision"]))
    assert all(abs(f-.45)<2e-9 for f in update["fields"][0]["vignetting"])
    assert all(b["limiting_endpoint"]=="image" and b["limiting_surface"] is None for b in update["fields"][0]["boundaries"])
    changed=tool("spot_diagram",args);spot.pop("revision");changed.pop("revision");assert changed==spot
    system=copy.deepcopy(initial["system"])
    system.update(object_distance=300,temperature=25,pupil_mapping="reference_entrance_pupil")
    system["object_surface"].update(radius=500)
    system["image_surface"].update(radius=100,conic=-1,even=[.0001])
    tool("system_set",dict(system=system,revision=tool("system_get")["revision"]))
    definition=dict(kind="edge_thickness",height=3,value=5)
    tool("solve",dict(gap=1,definition=definition,revision=tool("system_get")["revision"]))
    saved=tool("system_get")
    expected=5-50+math.sqrt(2491)-9/200-.0001*81
    assert abs(saved["system"]["surfaces"][1]["thickness"]-expected)<1e-11
    saved["system"]["image_surface"]["even"]=[.0002]
    tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
    saved=tool("system_get");expected=5-50+math.sqrt(2491)-9/200-.0002*81
    assert abs(saved["system"]["surfaces"][1]["thickness"]-expected)<1e-11
    psf=tool("diffraction_psf",dict(field_index=0,grid=16))
    assert abs(sum(psf["intensity"])-1)<1e-10 and "Planar lateral" in psf["image_sampling"]
    mtf=tool("diffraction_mtf",dict(field_index=0,grid=16,spectrum=dict(kind="polychromatic")))
    assert "Planar lateral" in mtf["image_sampling"] and len(mtf["components"])==3
    stored=(DATA/"system.json").read_bytes()
    for mutation in range(4):
        broken=copy.deepcopy(saved["system"])
        if mutation==0: del broken["object_surface"]
        elif mutation==1: broken["image_surface"]["thickness"]=2
        elif mutation==2: broken["image_surface"]["semi_diameter"]=-1
        else: broken["object_distance"]=None
        tool("project_import",dict(format="hissabeth",text=json.dumps(broken),revision=saved["revision"]),error=True)
        assert tool("system_get")==saved and (DATA/"system.json").read_bytes()==stored
    engine.terminate();engine.wait(timeout=5)
    stale=copy.deepcopy(saved["system"])
    stale["object_surface"]["semi_diameter"]=stale["image_surface"]["semi_diameter"]=999
    stale["surfaces"][1]["thickness"]=999
    (DATA/"system.json").write_text(json.dumps(stale));(DATA/"endpoint.json").unlink(missing_ok=True)
    engine=start_engine()
    recovered=tool("system_get")["system"]
    assert abs(recovered["surfaces"][1]["thickness"]-expected)<1e-11
    assert json.loads((DATA/"system.json").read_text())==recovered
    recovered["surfaces"][1]["thickness"]=saved["system"]["surfaces"][1]["thickness"]
    assert recovered==saved["system"]
    print("PASS: real MCP curved source/detector signed intersections and OPL, endpoint SPH/ASP/ODD exchange, object/image clipping and coverage, image-limited vignetting, final edge solve and profile edit reapplication, planar FFT model, atomic strict edits and same-relay stale-envelope/gap recovery")


def focus_overflow():
    import copy
    state=tool("system_get");system=state["system"]
    a=copy.deepcopy(system["surfaces"][0]);b=copy.deepcopy(system["surfaces"][1])
    a.update(radius=1,thickness=100,material="1.500000:60.000000",aperture=.1,conic=0,even=[],odd=[])
    b.update(radius=-1,thickness=10,material="AIR",aperture=.1,conic=0,even=[],odd=[])
    system.update(object_distance=1e308,surfaces=[a,b],stop_index=0,
                  fields=[dict(x=0,y=0,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])],
                  wavelengths=[dict(value=587.562,weight=1,color=[.7,.8,0,1])],reference_wavelength=0,
                  pupil=dict(kind="entrance_pupil_diameter",value=.001))
    tool("system_set",dict(system=system,revision=state["revision"]))
    before=tool("system_get");saved=(DATA/"system.json").read_bytes()
    for name,args in [("paraxial",{}),("spherochromatism",dict(samples=5)),
                      ("longitudinal_aberration",dict(samples=5)),
                      ("chromatic_focus",dict(lower=500,upper=650,samples=3))]:
        error=tool(name,args,error=True)
        assert "nonfinite" in error["content"][0]["text"].lower(),error
        assert tool("system_get")==before and (DATA/"system.json").read_bytes()==saved
    print("PASS: finite object-distance overflow returns errors for paraxial, spherochromatism, longitudinal and chromatic focus without killing the broker or mutating the project")


def silica_export():
    import copy
    state=tool("system_get");system=state["system"]
    system.update(object_distance=300,temperature=25,pupil_mapping="reference_entrance_pupil")
    for position in range(len(system["surfaces"])+1):
        trial=copy.deepcopy(system)
        if position==0: trial["object_material"]="FUSED_SILICA"
        else: trial["surfaces"][position-1]["material"]="FUSED_SILICA"
        tool("system_set",dict(system=trial,revision=tool("system_get")["revision"]))
        before=tool("system_get");saved=(DATA/"system.json").read_bytes()
        error=tool("project_export",dict(format="geopter"),error=True)
        assert "FUSED_SILICA" in error["content"][0]["text"]
        assert tool("system_get")==before and (DATA/"system.json").read_bytes()==saved
        assert json.loads(tool("project_export",dict(format="hissabeth"))["text"])==before["system"]
    print("PASS: real MCP rejects Geopter export for built-in silica in object and every surface medium without mutation; native export retains the prescription")


def project_metadata():
    import copy
    global engine
    state = tool("system_get")
    system = copy.deepcopy(state["system"])
    surface = copy.deepcopy(system["surfaces"][0])
    surface.update(radius=-50, thickness=123, material="AIR", aperture=20,
                   clear_aperture=True, conic=0, even=[], odd=[], gap_solve=dict(kind="fixed"))
    colors = [[.923456789012345, .123456789012345, .2, 0],
              [.1, .812345678901234, .2, .25], [.9, .7, .1, .5], [.1, .2, .9, 1]]
    waves = [dict(value=v, weight=w, color=c) for v, w, c in zip(
        [656.273, 587.562, 587.562, 486.133], [1, 1, 2, 0], colors)]
    fields = [dict(x=0, y=0, weight=1, vignetting=[0]*4, color=c) for c in colors[:2]]
    system.update(object_material="1.500000:60.000000", object_label="Submerged emitter",
                  image_label="Measured detector", object_distance=300, temperature=25,
                  pressure=101325, pupil_mapping="reference_entrance_pupil", surfaces=[surface],
                  stop_index=0, fields=fields, wavelengths=waves, reference_wavelength=1,
                  pupil=dict(kind="entrance_pupil_diameter", value=.0002))
    tool("system_set", dict(system=system, revision=state["revision"]))
    first = tool("paraxial")
    for key, expected in [("object_index", 1.5), ("efl", 100), ("image_distance", 200),
                          ("magnification", -1), ("front_focal_distance", -150)]:
        assert abs(first[key]-expected)<1e-9, (key, first[key])
    ray = tool("trace_ray", dict(x=0, y=.0001))
    last = ray["segments"][-1]["point"]
    focus = last[2]-last[1]*ray["direction"][2]/ray["direction"][1]
    assert abs(focus-200)<1e-4, focus
    expected = tool("system_get")["system"]
    for format in ("hissabeth", "geopter"):
        text = tool("project_export", dict(format=format))["text"]
        tool("project_import", dict(format=format, text=text, revision=tool("system_get")["revision"]))
        actual = tool("system_get")["system"]
        for key in ("object_material", "object_label", "image_label", "fields", "wavelengths"):
            assert actual[key]==expected[key], (format, key, actual[key], expected[key])
    poly = tool("polychromatic_spot", dict(field_index=0, sampling=dict(kind="hexapolar", rings=2)))
    assert len(poly["points"])==len(poly["colors"])==57
    for i, color in enumerate(colors[:3]):
        assert all(c==color for c in poly["colors"][19*i:19*(i+1)])
        assert poly["failures"][i]["color"]==color
    layout = tool("layout")
    assert layout["field_indices"]==[0]*7+[1]*7
    sphero = tool("spherochromatism", dict(samples=5))
    assert len(sphero["curves"])==4
    for i, curve in enumerate(sphero["curves"]):
        assert curve["wavelength_index"]==i and curve["color"]==colors[i]
        assert curve["weight"]==waves[i]["weight"] and curve["points"][0]["shift"]==0
    assert abs(sphero["curves"][0]["points"][0]["focus"]-sphero["reference_focus"])>1e-5
    tool("autofocus", dict(revision=tool("system_get")["revision"]))
    assert abs(tool("system_get")["system"]["surfaces"][0]["thickness"]-200)<1e-8
    definition = dict(kind="marginal_height", value=0, zone=0)
    tool("solve", dict(gap=0, definition=definition, revision=tool("system_get")["revision"]))
    state = tool("system_get")
    state["system"]["object_material"]="1.600000:60.000000"
    tool("system_set", dict(system=state["system"], revision=state["revision"]))
    state = tool("system_get")
    assert abs(state["system"]["surfaces"][0]["thickness"]-150)<1e-8
    stored = (DATA/"system.json").read_bytes()
    for mutation in range(5):
        broken = copy.deepcopy(state["system"])
        if mutation==0: del broken["object_material"]
        elif mutation==1: del broken["fields"][0]["color"]
        elif mutation==2: broken["wavelengths"][0]["color"]=[0,0,0,1.1]
        elif mutation==3: broken["object_material"]="UNKNOWN_OBJECT_MEDIUM"
        if mutation==4:
            tool("system_set", dict(system=broken, revision=state["revision"]-1), error=True)
        else:
            tool("project_import", dict(format="hissabeth", text=json.dumps(broken), revision=state["revision"]), error=True)
        assert tool("system_get")==state and (DATA/"system.json").read_bytes()==stored
    engine.terminate(); engine.wait(timeout=5)
    state["system"]["surfaces"][0]["thickness"]=999
    (DATA/"system.json").write_text(json.dumps(state["system"]))
    (DATA/"endpoint.json").unlink(missing_ok=True)
    engine = start_engine()
    recovered = tool("system_get")["system"]
    assert abs(recovered["surfaces"][0]["thickness"]-150)<1e-8
    assert recovered["object_material"]=="1.600000:60.000000"
    assert recovered["wavelengths"]==waves and recovered["fields"]==fields
    assert json.loads((DATA/"system.json").read_text())==recovered
    print("PASS: real MCP immersed Gaussian conjugates/rays, native and Geopter RGBA/labels/medium exchange, duplicate spectral/field identity, all-wavelength spherochromatism, autofocus and saved solve reapplication, atomic rejection and same-relay restart")


def maximum_payload():
    import copy
    state = tool("system_get"); system = state["system"]
    surface = copy.deepcopy(system["surfaces"][0])
    surface.update(radius=0, thickness=10, material="AIR", aperture=20,
                   clear_aperture=True, conic=0, even=[], odd=[], gap_solve=dict(kind="fixed"))
    radius = 1.234567891234567
    field = [1.234567891234567, -2.345678912345678]
    color = [.1234567890123456, .2345678901234567, .3456789012345678, .4567890123456789]
    system.update(surfaces=[surface, copy.deepcopy(surface)], object_material="AIR", object_distance=None,
                  stop_index=0, pupil_mapping="reference_entrance_pupil", reference_wavelength=0,
                  pupil=dict(kind="entrance_pupil_diameter", value=2*radius),
                  wavelengths=[dict(value=587.562123456789, weight=1, color=color) for _ in range(32)],
                  fields=[dict(x=field[0], y=field[1], weight=1, vignetting=[0]*4, color=color)])
    tool("system_set", dict(system=system, revision=state["revision"]))
    began = time.monotonic()
    result = tool("polychromatic_spot", dict(field_index=0, sampling=dict(kind="grid", count=129)))
    size = last_response_bytes
    pupils = [(i/64-1, j/64-1) for i in range(129) for j in range(129) if (i/64-1)**2+(j/64-1)**2<=1+1e-12]
    count = len(pupils)*32
    assert count==411296 and len(result["points"])==len(result["colors"])==len(result["weights"])==count
    assert size>64_000_000, size
    assert all(c==color for c in result["colors"])
    shift = [20*math.tan(math.radians(f)) for f in field]
    for index in (0, len(pupils)-1, count-1):
        xy = pupils[index%len(pupils)]
        assert max(abs(a-(radius*p+q)) for a,p,q in zip(result["points"][index], xy, shift))<1e-10
    assert max(abs(a-b) for a,b in zip(result["centroid"], shift))<1e-9
    rms = radius*math.sqrt(sum(x*x+y*y for x,y in pupils)/len(pupils))
    assert abs(result["rms_radius"]-rms)<1e-9
    assert abs(math.fsum(result["weights"])-32)<1e-10
    print(f"PASS: real stdio relay returned {size} bytes, {count} colored rays, complete arrays and independent plane/centroid/RMS checks in {time.monotonic()-began:.2f}s")


def fft_pupil():
    for mapping in ("real_stop","reference_entrance_pupil"):
        saved=tool("system_get");system=saved["system"]
        system.update(pupil_mapping=mapping,pupil=dict(kind="entrance_pupil_diameter",value=2),fields=[dict(x=.8,y=.6,weight=1,vignetting=[0]*4,color=[.2,.5,.8,1])])
        system["surfaces"][0]["aperture"]=.65
        tool("system_set",dict(system=system,revision=saved["revision"]))
        requests=[("spot_diagram",dict(field_index=0,sampling=dict(kind="hexapolar",rings=4))),("polychromatic_spot",dict(field_index=0,sampling=dict(kind="hexapolar",rings=4))),("encircled_energy",dict(field_index=0,sampling=dict(kind="hexapolar",rings=4),samples=9)),("geometric_mtf",dict(field_index=0,sampling=dict(kind="hexapolar",rings=4),spectrum=dict(kind="polychromatic"))),("wavefront",dict(field_index=0,grid=16)),("diffraction_psf",dict(field_index=0,grid=16)),("diffraction_mtf",dict(field_index=0,grid=16,spectrum=dict(kind="polychromatic")))]
        original=[tool(name,args) for name,args in requests]
        saved=tool("system_get");saved["system"]["fields"][0]["vignetting"]=[.4,.2,-.1,.3]
        tool("system_set",dict(system=saved["system"],revision=saved["revision"]))
        changed=[tool(name,args) for name,args in requests]
        for a,b in zip(original,changed):
            assert b.pop("revision")>a.pop("revision")
            assert a==b,(mapping,a.keys())
        assert changed[-1]["failed_pupil_rays"]>0
        assert changed[0]["blocked"]>0
    print("PASS: real MCP mono/poly spots, energy, geometric MTF, wavefront, PSF and diffraction MTF integrate nominal pupils on both mappings; physical-aperture losses and engine revisions retained")

try:
    initialized = request("initialize", dict(protocolVersion="2025-11-25", capabilities={}, clientInfo=dict(name="transport-test", version="1")))
    assert initialized["serverInfo"]["name"] == "hissabeth"
    relay.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
    relay.stdin.flush()
    tools = request("tools/list")["tools"]
    if "--workspace-recovery" in sys.argv:
        workspace_recovery()
        raise SystemExit(0)
    if "--malformed-parameters" in sys.argv:
        malformed_parameters()
        raise SystemExit(0)
    if "--artifacts" in sys.argv:
        assert {t["name"] for t in tools} >= {"artifact_start", "artifact_status", "artifact_fetch", "artifact_export", "artifact_cancel"}
        artifacts()
        raise SystemExit(0)
    setup_status=tool("workspace_status")
    if setup_status["state"]=="recovery":
        assert setup_status["source_text"] is None and setup_status["source_bytes"]==0
        tool("workspace_recover",dict(revision=setup_status["revision"],source=dict(kind="default")))
    if "--project-lifecycle" in sys.argv:
        project_lifecycle()
        raise SystemExit(0)
    if "--surface-edit" in sys.argv:
        surface_edit()
        raise SystemExit(0)
    if "--prescription-report" in sys.argv:
        prescription_report()
        raise SystemExit(0)
    if "--field-curvature" in sys.argv:
        field_curvature()
        raise SystemExit(0)
    if "--ray-inspection" in sys.argv:
        ray_inspection()
        raise SystemExit(0)
    if "--mirrors" in sys.argv:
        mirrors()
        raise SystemExit(0)
    if "--endpoints" in sys.argv:
        endpoints()
        raise SystemExit(0)
    if "--focus-overflow" in sys.argv:
        focus_overflow()
        raise SystemExit(0)
    if "--silica-export" in sys.argv:
        silica_export()
        raise SystemExit(0)
    if "--project-metadata" in sys.argv:
        project_metadata()
        raise SystemExit(0)
    if "--maximum-payload" in sys.argv:
        maximum_payload()
        raise SystemExit(0)
    if "--fft-pupil" in sys.argv:
        fft_pupil()
        raise SystemExit(0)
    if "--spectrum-sampling" in sys.argv:
        spectrum_sampling()
        raise SystemExit(0)
    if "--apertures" in sys.argv:
        assert {t["name"] for t in tools}>={"aperture_report","vignetting_update"}
        apertures()
        raise SystemExit(0)
    if "--persistent-solves" in sys.argv:
        assert {t["name"] for t in tools} >= {"solve", "solve_recalculate"}
        persistent_solves()
        raise SystemExit(0)
    assert {t["name"] for t in tools} >= {"system_get", "catalog_import", "wavefront", "diffraction_mtf", "project_import", "solve"}
    before = tool("system_get")
    revision = before["revision"]
    system = before["system"]
    system["name"] = "MCP transport recovery"
    tool("system_set", dict(system=system, revision=revision + 1), error=True)
    result = tool("system_set", dict(system=system, revision=revision))
    assert result["revision"] == revision + 1
    tool("spot_diagram", dict(sampling=dict(kind="grid",count=2)), error=True)
    index = tool("material_index", dict(material="N-BK7_SCHOTT", wavelength=587.5618))
    assert abs(index["index"] - 1.5168) < 1e-5
    missing = tool("material_index", dict(material="H-K9L_CDGM", wavelength=587.5618), error=True)
    assert "H-K9L is in the CDGM glass catalog, which is not installed" in missing["content"][0]["text"], missing
    vendors = {v["catalog"]: v for v in tool("catalog_list")["vendors"]}
    assert vendors["SCHOTT"]["installed"] and not vendors["CDGM"]["installed"], vendors
    assert "INFRARED" not in vendors and vendors["CDGM"]["page"].startswith("https://")
    spot = tool("spot_diagram", dict(sampling=dict(kind="grid",count=9), field_index=0))
    assert math.isfinite(spot["rms_radius"])
    psf = tool("diffraction_psf", dict(grid=16, field_index=0))
    assert abs(sum(psf["intensity"]) - 1.0) < 1e-10
    # Exercise registration using isolated CLI doubles; real account settings are untouched.
    fake_dir = ROOT / "target/agent-cli-fixtures"
    fake_dir.mkdir(exist_ok=True)
    fake_program = r"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
base = Path(os.environ["HISSABETH_AGENT_TEST_ROOT"])
state = base / (".claude.json" if name == "claude" else "codex.json")
entries = json.loads(state.read_text())["mcpServers"] if state.exists() else {}
with (base / "commands.jsonl").open("a") as log:
    log.write(json.dumps({"name": name, "args": sys.argv[1:]}) + "\n")
operation = sys.argv[2]
if operation == "list":
    print(json.dumps([{"name": key, "transport": value} for key, value in entries.items()]))
elif operation == "remove":
    entries.pop(sys.argv[-1], None)
    state.write_text(json.dumps({"mcpServers": entries}))
elif operation == "add":
    arguments = sys.argv[1:]
    separator = arguments.index("--")
    command = arguments[separator+1]
    env_key, env_value = arguments[arguments.index("--env") + 1].split("=", 1)
    entries[arguments[2]] = {"command": command, "args": arguments[separator+2:], "env": {env_key: env_value}}
    state.write_text(json.dumps({"mcpServers": entries}))
    print("Registered")
else:
    sys.exit(2)
"""
    for name in ("codex", "claude"):
        file = fake_dir / name
        file.write_text(fake_program)
        file.chmod(0o755)
        (fake_dir / (".claude.json" if name == "claude" else "codex.json")).unlink(missing_ok=True)
    (fake_dir / "commands.jsonl").unlink(missing_ok=True)
    registration_env = dict(ENV, PATH=str(fake_dir) + os.pathsep + os.environ["PATH"], HISSABETH_AGENT_TEST_ROOT=str(fake_dir), CLAUDE_CONFIG_DIR=str(fake_dir))
    for _ in range(2):
        registration = json.loads(subprocess.run([str(BINARY), "--configure-agents"], env=registration_env, capture_output=True, text=True, check=True, timeout=15).stdout)
        assert len(registration) == 2 and all(r["success"] for r in registration), registration
    commands = [json.loads(line) for line in (fake_dir / "commands.jsonl").read_text().splitlines()]
    additions = [c for c in commands if c["args"][1] == "add"]
    assert len(additions) == 2, commands
    for addition in additions:
        assert "HISSABETH_DATA_DIR=" + str(DATA) in addition["args"]
        assert addition["args"][-2:] == [str(BINARY), "--mcp"]
    print("PASS: isolated Codex/Claude CLI registration and repeated registration")
    # A second broker must not acquire the same optical workspace.
    duplicate = subprocess.run([str(BINARY), "--engine"], env=ENV, capture_output=True, text=True, timeout=5)
    assert duplicate.returncode != 0
    # Unauthenticated connections cannot dispatch a command.
    endpoint = json.loads((DATA / "endpoint.json").read_text())
    host, port = endpoint["address"].rsplit(":", 1)
    with socket.create_connection((host, int(port)), timeout=5) as connection:
        connection.sendall((json.dumps(dict(token="wrong", request=dict(jsonrpc="2.0", id=1, method="ping"))) + "\n").encode())
        assert not connection.recv(1024)
    engine.terminate()
    engine.wait(timeout=5)
    (DATA / "endpoint.json").unlink()
    engine = start_engine()
    # The existing relay reconnects to the new endpoint and sees the saved prescription.
    recovered = tool("system_get")
    assert recovered["system"]["name"] == "MCP transport recovery"
    assert recovered["revision"] != revision
    tool("system_set", dict(system=system, revision=revision), error=True)
    print(f"PASS: {len(tools)} tools; real broker/stdio handshake, validation, save, authentication, exclusive workspace, and restart recovery")
finally:
    relay.stdin.close()
    relay.terminate()
    relay.wait(timeout=5)
    engine.terminate()
    engine.wait(timeout=5)
