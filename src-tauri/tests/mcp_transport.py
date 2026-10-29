"""Exercise the real headless broker and stdio relay, including saved-state recovery."""
import json
import math
import os
from pathlib import Path
import socket
import select
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hissabeth"
DATA = ROOT / "target/transport-workspace"
DATA.mkdir(exist_ok=True)
for filename in ("system.json", "endpoint.json"):
    (DATA / filename).unlink(missing_ok=True)
ENV = dict(os.environ, HISSABETH_DATA_DIR=str(DATA))


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


def request(method, params=None):
    global identifier, last_response_bytes
    identifier += 1
    message = dict(jsonrpc="2.0", id=identifier, method=method)
    if params is not None:
        message["params"] = params
    relay.stdin.write(json.dumps(message) + "\n")
    relay.stdin.flush()
    if not select.select([relay.stdout], [], [], 20)[0]:
        raise TimeoutError(f"No response for {method}")
    line = relay.stdout.readline()
    last_response_bytes = len(line.encode())
    response = json.loads(line)
    assert response["id"] == identifier, response
    assert "error" not in response, response
    return response["result"]


def tool(name, args=None, error=False):
    result = request("tools/call", dict(name=name, arguments=args or {}))
    assert result["isError"] is error, result
    return result if error else json.loads(result["content"][0]["text"])


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
    # Run numerical checks only for the eight upstream examples not previously compared.
    source=ROOT.parent/"reference/upstream/example"
    catalogs=[p for p in (ROOT/"data/glass").iterdir() if p.suffix.lower()==".agf"]
    for path in source.rglob("*.json"):
        if path.name in ("sasian_triplet.json","kingslake_doublet.json"): continue
        current=tool("system_get")
        tool("project_import",dict(format="geopter",text=path.read_text(),revision=current["revision"]))
        imported=tool("system_get")
        first=tool("paraxial")
        oracle=json.loads(subprocess.run([str(ROOT/"target/geopter_oracle"),str(path),*map(str,catalogs)],capture_output=True,text=True,check=True,timeout=20).stdout)
        for key in ("efl","bfl","entrance_pupil_distance","entrance_pupil_radius","exit_pupil_distance","exit_pupil_radius"):
            actual=first[key]; expected=oracle[key]
            assert abs(actual-expected)<=2e-5*max(1,abs(expected)),(path.name,key,actual,expected)
        for index,expected in enumerate(oracle["chief_rays"]):
            assert expected is not None,(path.name,index,"upstream chief failed")
            ray=tool("trace_ray",dict(field_index=index,x=0,y=0))
            actual=ray["points"][-1][:2]
            assert max(abs(a-b) for a,b in zip(actual,expected))<2e-4,(path.name,index,actual,expected)
        print("PASS: upstream first-order/chief comparison:",path.relative_to(source))
    print("PASS: aperture/vignetting MCP revision, sizing, factors, native persistence, atomic rejection, clear and same-relay corrected-size restart recovery")


def prior_chiefs():
    # The trace launch and signed intersections changed; reuse unchanged first-order
    # evidence and compare only the two earlier prescriptions' affected chief rays.
    source=ROOT.parent/"reference/upstream/example/book"
    for filename in ("sasian_triplet.json","kingslake_doublet.json","sasian_triplet_glass.json"):
        path=source/filename
        if filename=="sasian_triplet.json":
            # Upstream Buchdahl's quadratic denominator has the wrong sign:
            # its off-reference indices do not reproduce the prescribed Abbe number.
            # Compare the mapping at d, and use catalog glass for the full spectrum.
            project=json.loads(path.read_text());waves=project["Spec"]["Wvl"];ref=waves["RefIndex"]
            for key in ("Value","Weight","Color"): waves[key]=[waves[key][ref]]
            waves["RefIndex"]=0
            path=ROOT/"target/sasian-reference-only.json";path.write_text(json.dumps(project))
        current=tool("system_get")
        tool("project_import",dict(format="geopter",text=path.read_text(),revision=current["revision"]))
        oracle=json.loads(subprocess.run([str(ROOT/"target/geopter_oracle"),str(path)],capture_output=True,text=True,check=True,timeout=20).stdout)
        for index,expected in enumerate(oracle["chief_rays"]):
            assert expected is not None,(filename,index)
            actual=tool("trace_ray",dict(field_index=index,x=0,y=0))["points"][-1][:2]
            assert max(abs(a-b) for a,b in zip(actual,expected))<2e-4,(filename,index,actual,expected)
        print("PASS: affected current chief rays; earlier first-order evidence reused:",filename)


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
    # Exercise the original C++ analysis at the same physical frequencies and
    # grid rays, with the original equal-weight, zero-vignetting prescriptions.
    source=ROOT.parent/"reference/upstream/example/book"
    for filename in ("sasian_triplet.json","kingslake_doublet.json","sasian_triplet_glass.json"):
        path=source/filename
        if filename=="sasian_triplet.json":
            # Upstream Buchdahl's quadratic denominator has the wrong sign:
            # its off-reference indices do not reproduce the prescribed Abbe number.
            # Compare the mapping at d, and use catalog glass for the full spectrum.
            project=json.loads(path.read_text());waves=project["Spec"]["Wvl"];ref=waves["RefIndex"]
            for key in ("Value","Weight","Color"): waves[key]=[waves[key][ref]]
            waves["RefIndex"]=0
            path=ROOT/"target/sasian-reference-only.json";path.write_text(json.dumps(project))
        current=tool("system_get")
        tool("project_import",dict(format="geopter",text=path.read_text(),revision=current["revision"]))
        imported=tool("system_get")
        assert imported["system"]["pupil_mapping"]=="reference_entrance_pupil"
        native=json.loads(tool("project_export",dict(format="hissabeth"))["text"])
        assert native["pupil_mapping"]=="reference_entrance_pupil"
        oracle=json.loads(subprocess.run([str(ROOT/"target/geopter_oracle"),"--geometric",str(path),str(ROOT.parent/"reference/upstream/data/AGF/SCHOTT.agf")],capture_output=True,text=True,check=True,timeout=30).stdout)
        assert len(oracle["curves"])>=2
        maximum_error=0.
        for curve in oracle["curves"]:
            field=int(curve["name"].split("_")[0][1:]);axis="x" if curve["name"].endswith("_S") else "y"
            result=tool("geometric_mtf",dict(field_index=field,sampling=dict(kind="grid",count=17),spectrum=dict(kind="polychromatic"),axis=axis,max_frequency=100))
            assert result["frequencies"]==curve["frequencies"]
            delta=max(abs(a-b) for a,b in zip(result["mtf"],curve["values"]))
            maximum_error=max(maximum_error,delta)
            assert delta<.002,(filename,curve["name"],delta)
        print("PASS: original C++ geometric MTF, all fields/axes/65 frequencies:",filename,"max absolute error",maximum_error)
    print("PASS: weighted hexapolar spot/energy, strict spectral/sampling arguments, real lateral-color OTF cancellation, diffraction physical frequencies/complex components, skipped zero weights, positive-weight failure without mutation, engine analysis revisions")


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
    path=ROOT.parent/"reference/upstream/example/book/sasian_triplet_glass.json"
    project=json.loads(path.read_text());fields=project["Spec"]["Field"]
    for key,value in (("VUY",.4),("VLY",.2),("VUX",-.1),("VLX",.3)):
        fields[key]=[value]*len(fields["Y"])
    path=ROOT/"target/sasian-catalog-nonzero-vignetting.json";path.write_text(json.dumps(project))
    saved=tool("system_get");tool("project_import",dict(format="geopter",text=path.read_text(),revision=saved["revision"]))
    oracle=json.loads(subprocess.run([str(ROOT/"target/geopter_oracle"),"--geometric",str(path),str(ROOT.parent/"reference/upstream/data/AGF/SCHOTT.agf")],capture_output=True,text=True,check=True,timeout=30).stdout)
    maximum=0.
    for curve in oracle["curves"]:
        field=int(curve["name"].split("_")[0][1:]);axis="x" if curve["name"].endswith("_S") else "y"
        result=tool("geometric_mtf",dict(field_index=field,sampling=dict(kind="grid",count=17),spectrum=dict(kind="polychromatic"),axis=axis,max_frequency=100))
        delta=max(abs(a-b) for a,b in zip(result["mtf"],curve["values"]))
        assert delta<.002,(curve["name"],delta)
        maximum=max(maximum,delta)
    print("PASS: original C++ full-spectrum geometric MTF with asymmetric nonzero stored factors, all fields/global axes/65 frequencies, max absolute error",maximum)
    print("PASS: real MCP mono/poly spots, energy, geometric MTF, wavefront, PSF and diffraction MTF integrate nominal pupils on both mappings; physical-aperture losses and engine revisions retained")

try:
    initialized = request("initialize", dict(protocolVersion="2025-11-25", capabilities={}, clientInfo=dict(name="transport-test", version="1")))
    assert initialized["serverInfo"]["name"] == "hissabeth"
    relay.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
    relay.stdin.flush()
    tools = request("tools/list")["tools"]
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
    if "--prior-chiefs" in sys.argv:
        prior_chiefs()
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
    spot = tool("spot_diagram", dict(sampling=dict(kind="grid",count=9), field_index=0))
    assert math.isfinite(spot["rms_radius"])
    psf = tool("diffraction_psf", dict(grid=16, field_index=0))
    assert abs(sum(psf["intensity"]) - 1.0) < 1e-10
    oracle = ROOT / "target/geopter_oracle"
    if oracle.exists():
        for filename in ("sasian_triplet.json", "kingslake_doublet.json"):
            fixture = ROOT.parent / "reference/upstream/example/book" / filename
            reference = json.loads(subprocess.run([str(oracle), str(fixture)], capture_output=True, text=True, check=True, timeout=20).stdout)
            current = tool("system_get")
            tool("project_import", dict(format="geopter", text=fixture.read_text(), revision=current["revision"]))
            first_order = tool("paraxial")
            for key in ("efl", "bfl", "entrance_pupil_distance", "entrance_pupil_radius", "exit_pupil_distance", "exit_pupil_radius"):
                actual = first_order[key]
                expected = reference[key]
                assert abs(actual - expected) < 2e-5 * max(1, abs(expected)), (filename, key, actual, expected)
            for index, expected in enumerate(reference["chief_rays"]):
                if expected is None:
                    continue
                actual = tool("trace_ray", dict(field_index=index))["points"][-1]
                assert math.hypot(actual[0] - expected[0], actual[1] - expected[1]) < 2e-4, (filename, index, actual, expected)
            print("PASS: C++ Geopter first-order and chief-ray comparison:", filename)
        current = tool("system_get")
        tool("system_set", dict(system=system, revision=current["revision"]))
    # Exercise registration using isolated CLI doubles; real account settings are untouched.
    fake_dir = ROOT / "target/agent-cli-fixtures"
    fake_dir.mkdir(exist_ok=True)
    fake_program = r"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
base = Path(os.environ["HISSABETH_AGENT_TEST_ROOT"])
state = base / (name + ".json")
with (base / "commands.jsonl").open("a") as log:
    log.write(json.dumps({"name": name, "args": sys.argv[1:]}) + "\n")
operation = sys.argv[2]
if operation == "get":
    if not state.exists(): sys.exit(1)
    print(state.read_text())
elif operation == "remove":
    state.unlink(missing_ok=True)
elif operation == "add":
    arguments = sys.argv[1:]
    separator = arguments.index("--")
    command = arguments[separator+1]
    state.write_text(json.dumps({"command": command, "args": arguments[separator+2:], "env": arguments[arguments.index("--env")+1]}))
    print("Registered")
else:
    sys.exit(2)
"""
    for name in ("codex", "claude"):
        file = fake_dir / name
        file.write_text(fake_program)
        file.chmod(0o755)
        (fake_dir / (name + ".json")).unlink(missing_ok=True)
    (fake_dir / "commands.jsonl").unlink(missing_ok=True)
    registration_env = dict(ENV, PATH=str(fake_dir) + os.pathsep + os.environ["PATH"], HISSABETH_AGENT_TEST_ROOT=str(fake_dir))
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
