# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Real matrix selection, geometry, popup and shared-URL regressions."""
from __future__ import annotations

import shutil
from pathlib import Path

import pytest

selenium = pytest.importorskip("selenium")
from selenium import webdriver  # noqa: E402
from selenium.webdriver.chrome.options import Options  # noqa: E402
from selenium.webdriver.common.action_chains import ActionChains  # noqa: E402
from selenium.webdriver.common.by import By  # noqa: E402
from selenium.webdriver.common.keys import Keys  # noqa: E402
from selenium.webdriver.support.ui import WebDriverWait  # noqa: E402

pytestmark = pytest.mark.skipif(
    not any(shutil.which(binary) for binary in ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser")),
    reason="no headless Chrome available",
)


@pytest.fixture(scope="module")
def matrix_driver():
    options = Options()
    for argument in ("--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage", "--window-size=2400,1200"):
        options.add_argument(argument)
    driver = webdriver.Chrome(options=options)
    yield driver
    driver.quit()


def load(driver, rendered_page: Path, query=""):
    driver.get(rendered_page.resolve().as_uri() + query)
    WebDriverWait(driver, 20).until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .comparison-matrix"))


def selection(driver):
    return driver.execute_script("const s=document.querySelector('.tab-panel.active')._comparisonSelection;return {keys:s.keys,reference:s.reference}")


def click(driver, selector):
    driver.find_element(By.CSS_SELECTOR, ".tab-panel.active " + selector).click()


def choose_slot(driver, slot, engine, capture=None):
    click(driver, f".engine-card [data-slot-picker='{slot}']")
    driver.execute_script("const e=document.querySelector('dialog[open] [data-picker-engine]');e.value=arguments[0];e.dispatchEvent(new Event('change',{bubbles:true}));", engine)
    if capture:
        driver.execute_script("document.querySelector('dialog[open] [data-picker-version]').value=arguments[0]", capture)
    driver.find_element(By.CSS_SELECTOR, "dialog[open] [data-picker-apply]").click()


def test_geometry_focus_orientation_and_themes(matrix_driver, rendered_page):
    d = matrix_driver
    load(d, rendered_page)
    assert d.find_element(By.CSS_SELECTOR, ".tab-button.active").get_attribute("data-tab-target") == "tab-unified"
    assert len(d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .engine-card")) == 3
    assert "matches GOLDEN" in d.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text
    assert not d.find_elements(By.CSS_SELECTOR, ".model-summary-bar")
    labels = d.execute_script("""
      const p=document.querySelector('.tab-panel.active'),tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id);
      return {actual:[...p.querySelectorAll('.matrix-case-heading')].map(n=>{const c=n.cloneNode(true);c.querySelector('.ttip')?.remove();return c.textContent.trim()}),
        expected:tab.columns.map(c=>c.compact_prefix+'-'+c.display_number)};
    """)
    assert labels["actual"] == labels["expected"]
    d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
    WebDriverWait(d, 10).until(lambda browser: "layout-vertical" in browser.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class"))
    vertical_labels = d.execute_script("""
      const p=document.querySelector('.tab-panel.active'),tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id);
      const headings=[...p.querySelectorAll('.matrix-case-heading')];
      const sample=headings[1],small=sample.querySelector('.case-label-placeholder');
      return {actual:headings.map(n=>{const c=n.cloneNode(true);c.querySelector('.ttip')?.remove();return c.textContent.trim()}),
        expected:tab.columns.map(c=>c.label+' ('+c.compact_prefix+'-'+c.display_number+')'),
        small:small&&{color:getComputedStyle(small).color,fontSize:parseFloat(getComputedStyle(small).fontSize),parentSize:parseFloat(getComputedStyle(sample).fontSize)}};
    """)
    assert vertical_labels["actual"] == vertical_labels["expected"]
    assert vertical_labels["small"]["color"] == "rgb(138, 143, 152)" and vertical_labels["small"]["fontSize"] < vertical_labels["small"]["parentSize"], vertical_labels
    d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
    WebDriverWait(d, 10).until(lambda browser: "layout-vertical" not in browser.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class"))
    load(d, rendered_page, "?tab=tab-toolcalling-batch")
    labels = d.execute_script("""
      const p=document.querySelector('.tab-panel.active'),tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id);
      return {actual:[...p.querySelectorAll('.matrix-case-heading')].map(n=>{const c=n.cloneNode(true);c.querySelector('.ttip')?.remove();return c.textContent.trim()}),
        expected:tab.columns.map(c=>c.display_number||c.label)};
    """)
    assert labels["actual"] == labels["expected"]
    for theme in ("light", "dark"):
        d.execute_script("document.documentElement.dataset.theme=arguments[0]", theme)
        geometry = d.execute_script("""
          const p=document.querySelector('.tab-panel.active');
          const r=p.querySelector('.comparison-matrix-card').getBoundingClientRect();
          return {left:r.left,right:document.documentElement.clientWidth-r.right,tab:getComputedStyle(document.querySelector('.tab-button.active')).backgroundColor,
            tabs:getComputedStyle(document.querySelector('.tabs')).backgroundColor,page:getComputedStyle(document.body).backgroundColor};
        """)
        assert geometry["left"] == pytest.approx(12)
        assert geometry["right"] == pytest.approx(12)
        assert geometry["tab"] == geometry["tabs"] == geometry["page"]
        click(d, "[data-layout='stacked']")
        tints = d.execute_script("return [...document.querySelectorAll('.tab-panel.active .stacked-row')].slice(0,3).map(row=>[...row.children].map(cell=>getComputedStyle(cell).backgroundColor));")
        assert len({row[0] for row in tints}) == 3, [sorted(set(row)) for row in tints]
        assert all(len(set(row)) == 1 for row in tints)
        family_border = d.execute_script("""
          const row=document.querySelector('.tab-panel.active .matrix-model-row.family-start');
          const style=getComputedStyle(row.firstElementChild);
          return {count:document.querySelectorAll('.tab-panel.active .matrix-model-row.family-start').length,width:style.borderTopWidth,color:style.borderTopColor};
        """)
        assert family_border["count"] > 1 and family_border["width"] == "2px", family_border
        assert family_border["color"] == ("rgb(17, 24, 39)" if theme == "light" else "rgb(232, 237, 243)"), family_border
        score = d.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text
        click(d, "[data-focus='disagreements']")
        cells = d.execute_script("return [...document.querySelectorAll('.tab-panel.active .matrix-cell')].map(c=>({r:c.dataset.row,c:c.dataset.column,disagree:c.dataset.disagreement==='true',dim:c.classList.contains('focus-dim')}));")
        assert any(cell["disagree"] for cell in cells)
        assert all(cell["dim"] != cell["disagree"] for cell in cells)
        assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text == score
        d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
        WebDriverWait(d, 10).until(lambda browser: "layout-vertical" in browser.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class"))
        assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-layout='stacked']").text == "Stacked columns"
        assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active .legacy-report-details").get_dom_attribute("open") is None
        vertical = d.execute_script("return [...document.querySelectorAll('.tab-panel.active .matrix-cell')].map(c=>({r:c.dataset.row,c:c.dataset.column,disagree:c.dataset.disagreement==='true',dim:c.classList.contains('focus-dim')}));")
        assert sorted(cells, key=lambda x: (x["r"], x["c"])) == sorted(vertical, key=lambda x: (x["r"], x["c"]))
        vertical_border = d.execute_script("""
          const cell=document.querySelector('.tab-panel.active .comparison-matrix td.family-start');
          const style=getComputedStyle(cell);
          return {count:document.querySelectorAll('.tab-panel.active .comparison-matrix td.family-start').length,width:style.borderLeftWidth,color:style.borderLeftColor};
        """)
        assert vertical_border["count"] > 1 and vertical_border["width"] == "2px", vertical_border
        assert vertical_border["color"] == ("rgb(17, 24, 39)" if theme == "light" else "rgb(232, 237, 243)"), vertical_border
        click(d, "[data-layout='split']")
        vertical_width = d.execute_script("""
          const scroll=document.querySelector('.tab-panel.active .comparison-matrix-scroll');
          const table=scroll.querySelector('.comparison-matrix');
          const headers=[...table.querySelectorAll('thead .matrix-case-row th')].slice(1);
          return {tableMinWidth:getComputedStyle(table).minWidth,headerMinWidths:headers.map(n=>getComputedStyle(n).minWidth)};
        """)
        assert vertical_width["tableMinWidth"] == "max-content", vertical_width
        assert vertical_width["headerMinWidths"] and set(vertical_width["headerMinWidths"]) == {"0px"}, vertical_width
        assert len(d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-segments")) * 3 == len(cells)
        d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
    d.set_window_size(540, 900)
    assert d.execute_script("return document.documentElement.scrollWidth <= innerWidth + 1")
    d.set_window_size(2400, 1200)
    assert not [entry for entry in d.get_log("browser") if entry["level"] == "SEVERE"]


def test_interchangeable_slots_reference_empty_and_legacy_urls(matrix_driver, rendered_page):
    d = matrix_driver
    load(d, rendered_page)
    defaults = selection(d)
    click(d, ".engine-card .engine-mark")
    assert d.find_elements(By.CSS_SELECTOR, "dialog[open]")
    placement = d.execute_script("""
      const trigger=document.querySelector('.tab-panel.active .engine-card .engine-mark').closest('[data-slot-picker]');
      const a=trigger.getBoundingClientRect(),p=document.querySelector('dialog[open]').getBoundingClientRect();
      return {horizontal:Math.min(Math.abs(p.left-a.right),Math.abs(a.left-p.right)),overlapsVertically:p.top<a.bottom&&p.bottom>a.top};
    """)
    assert placement["horizontal"] <= 12, placement
    assert placement["overlapsVertically"], placement
    d.find_element(By.CSS_SELECTOR, "dialog[open] button[value='cancel']").click()
    click(d, ".engine-card [data-slot-picker='2']")
    missing = d.execute_script("return [...document.querySelectorAll('dialog[open] [data-picker-version] option')].filter(o=>o.textContent.includes('no captured results')).map(o=>({key:o.value,disabled:o.disabled}))")
    assert missing and all(item["disabled"] for item in missing)
    d.find_element(By.CSS_SELECTOR, "dialog[open] button[value='cancel']").click()
    click(d, "[data-slot-reference='2']")
    changed = selection(d)
    assert changed["keys"] == defaults["keys"]
    d.refresh()
    assert selection(d) == changed
    versions = d.execute_script("return window.__CONFORMANCE_PAGE.tabs.find(t=>t.id==='tab-unified').candidates.filter(c=>c.impl.startsWith('dynamo')).map(c=>c.key)")
    other = next(key for key in versions if key not in changed["keys"])
    choose_slot(d, 1, "dynamo", other)
    assert selection(d)["keys"][:2] == [defaults["keys"][0], other]
    assert selection(d)["reference"] == defaults["keys"][1]
    choose_slot(d, 2, "none")
    assert selection(d)["reference"] == defaults["keys"][0]
    click(d, ".engine-card [data-slot-picker='2']")
    d.execute_script("const e=document.querySelector('dialog[open] [data-picker-engine]');e.value='dynamo';e.dispatchEvent(new Event('change',{bubbles:true}));")
    disabled = d.execute_script("return [...document.querySelectorAll('dialog[open] [data-picker-version] option:disabled')].map(o=>o.value)")
    assert set(selection(d)["keys"][:2]) <= set(disabled)
    d.find_element(By.CSS_SELECTOR, "dialog[open] button[value='cancel']").click()
    for slot in range(3):
        choose_slot(d, slot, "none")
    assert selection(d) == {"keys": [], "reference": None}
    assert not d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-status")
    assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-matrix").get_attribute("data-empty") == "true"
    d.refresh()
    assert selection(d) == {"keys": [], "reference": None}
    choose_slot(d, 2, "dynamo", other)
    assert selection(d) == {"keys": [other], "reference": other}
    click(d, "[data-focus='disagreements']")
    assert d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell[data-disagreement='true']:not(.focus-dim)")
    keys = [versions[1], versions[0], defaults["keys"][1], versions[2]]
    query = "?tab=tab-unified&transpose=1&base_tab-unified=" + keys[0] + "&cmp_tab-unified=" + ",".join(keys[1:])
    load(d, rendered_page, query)
    assert selection(d)["keys"] == keys[:3]
    assert "three-slot limit" in d.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-notice").text
    assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active .legacy-report-details").get_dom_attribute("open") is None
    load(d, rendered_page)
    choose_slot(d, 0, "none")
    click(d, ".engine-card [data-slot-picker='0']")
    unavailable = d.execute_script("return [...document.querySelectorAll('dialog[open] [data-picker-engine] option:disabled')].map(o=>o.textContent)")
    assert any("SGLang" in label and "no captures" in label for label in unavailable)
    d.find_element(By.CSS_SELECTOR, "dialog[open] button[value='cancel']").click()
    load(d, rendered_page, "?tab=tab-toolcalling-batch")
    modes = d.execute_script("return window.__CONFORMANCE_PAGE.tabs.find(t=>t.id==='tab-toolcalling-batch').candidates.filter(c=>c.impl.startsWith('dynamo')).map(c=>({key:c.key,mode:c.parse_mode}))")
    first = modes[0]
    alternate = next(item for item in modes if item["mode"] != first["mode"])
    choose_slot(d, 0, "dynamo", first["key"])
    choose_slot(d, 1, "dynamo", alternate["key"])
    selected = selection(d)
    for layout in ("split", "stacked"):
        click(d, f"[data-layout='{layout}']")
        for vertical in (True, False):
            d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
            WebDriverWait(d, 10).until(lambda browser: ("layout-vertical" in browser.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class")) == vertical)
            assert selection(d) == selected
            d.refresh()
            assert selection(d) == selected


def test_shared_url_categories_controls_tabs_and_reset(matrix_driver, rendered_page):
    d = matrix_driver
    load(d, rendered_page)
    click(d, ".category-chip")
    click(d, "[data-layout='stacked']")
    click(d, "[data-focus='disagreements']")
    click(d, "[data-peer-only]")
    d.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-case-search]").send_keys("tool_only")
    d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
    before = d.execute_script("return document.querySelector('.tab-panel.active .comparison-ui').innerHTML")
    d.refresh()
    assert d.execute_script("return document.querySelector('.tab-panel.active .comparison-ui').innerHTML") == before
    d.execute_script("while(document.querySelector('.tab-panel.active .category-chip[aria-pressed=true]')) document.querySelector('.tab-panel.active .category-chip[aria-pressed=true]').click();")
    assert "groups_tab-unified=" in d.current_url
    d.refresh()
    assert not d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip[aria-pressed='true']")
    assert not d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell")
    d.find_element(By.CSS_SELECTOR, "[data-tab-target='tab-toolcalling-batch']").click()
    assert d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip[aria-pressed='true']")
    d.find_element(By.CSS_SELECTOR, "[data-tab-target='tab-unified']").click()
    assert not d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip[aria-pressed='true']")
    d.find_element(By.CSS_SELECTOR, "[data-reset]").click()
    WebDriverWait(d, 20).until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip[aria-pressed='true']"))
    assert d.current_url.endswith("?tab=tab-unified")
    assert not d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").is_selected()
    assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-layout='split']").get_attribute("aria-pressed") == "true"


def test_cell_and_case_popup_renderer_parity_and_drawer(matrix_driver, rendered_page):
    d = matrix_driver
    load(d, rendered_page)
    # Folded cases exercise variants; case-header tables exercise schemas and all families.
    result = d.execute_script("""
      const panel=document.querySelector('.tab-panel.active');
      const tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===panel.id);
      const rows=tab.rows.filter(r=>!r.section);
      const cell=[...panel.querySelectorAll('.matrix-cell')].find(n=>rows[+n.dataset.row].cells[tab.columns[+n.dataset.column].sub]?.tooltip?.variants);
      cell.focus();window.__buildTooltip(cell);
      const expected=document.createElement('div');
      expected.innerHTML='<div class="ttip">'+window.__renderConformanceTooltip(rows[+cell.dataset.row].cells[tab.columns[+cell.dataset.column].sub].tooltip,rows[+cell.dataset.row].cells[tab.columns[+cell.dataset.column].sub].fixture_href)+'</div>';
      window.__syncConformanceTooltip(expected,panel._comparisonSelection.keys,panel._comparisonSelection.reference,true);
      const tip=cell._ttip;const actual=tip.cloneNode(true);actual.querySelector('.ttip-close')?.remove();
      return {same:expected.querySelector('.ttip').textContent===actual.textContent,variants:tip.querySelectorAll('.case-variant').length,
        golden:tip.querySelectorAll('.cand-golden.cand-on,[data-cand=golden]:not(.col-hidden),.golden-out-cap,.oracle-output').length,fixtures:tip.querySelectorAll('a[href]').length};
    """)
    assert result["same"] and result["variants"] > 1 and result["golden"], result
    ActionChains(d).move_to_element(d.find_element(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell")).perform()
    WebDriverWait(d, 10).until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".ttip.ttip-visible"))
    d.execute_script("const c=document.querySelector('.tab-panel.active .matrix-cell');c.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch'}));c.click();")
    WebDriverWait(d, 10).until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".comparison-drawer.open"))
    assert not d.find_elements(By.CSS_SELECTOR, ".ttip-pinned")
    assert d.find_elements(By.CSS_SELECTOR, ".comparison-drawer.open .cand-golden.cand-on,.comparison-drawer.open [data-cand=golden]:not(.col-hidden),.comparison-drawer.open .golden-out-cap,.comparison-drawer.open .oracle-output")
    d.switch_to.active_element.send_keys(Keys.ESCAPE)
    for vertical in (False, True):
        if vertical:
            d.find_element(By.CSS_SELECTOR, "[data-transpose-toggle]").click()
            WebDriverWait(d, 10).until(lambda browser: "layout-vertical" in browser.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class"))
        grammar = d.execute_script("""
          const p=document.querySelector('.tab-panel.active');
          const tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id);
          const header=[...p.querySelectorAll('.matrix-case-heading')].find(n=>tab.columns[+n.dataset.caseColumn].schemas?.length);
          const chosen=header || p.querySelector('.matrix-case-heading');
          chosen.focus();window.__buildTooltip(chosen);
          const model=window.__conformanceCaseTooltip(tab,tab.columns[+chosen.dataset.caseColumn]);
          const expected=document.createElement('div');expected.innerHTML='<div class="ttip">'+window.__renderConformanceTooltip(model)+'</div>';
          window.__syncConformanceTooltip(expected,p._comparisonSelection.keys,p._comparisonSelection.reference,true);
          const tip=chosen._ttip;const actual=tip.cloneNode(true);actual.querySelector('.ttip-close')?.remove();
          window.__caseHeader=chosen;
          return {same:actual.textContent===expected.querySelector('.ttip').textContent,
            families:tip.querySelectorAll('table.ttip-grammar tbody tr').length,golden:tip.querySelectorAll('[data-cand=golden]:not(.col-hidden)').length};
        """)
        assert grammar["same"] and grammar["families"] > 1 and grammar["golden"], grammar
        d.execute_script("const c=window.__caseHeader;c.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch'}));c.click();")
        assert d.find_elements(By.CSS_SELECTOR, ".ttip-pinned")
        d.find_element(By.CSS_SELECTOR, ".ttip-pinned .ttip-close").click()
        assert not d.find_elements(By.CSS_SELECTOR, ".ttip-pinned")
    load(d, rendered_page, "?tab=tab-toolcalling-streamv1")
    chunks = d.execute_script("""
      const p=document.querySelector('.tab-panel.active');
      const tab=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id),rows=tab.rows.filter(r=>!r.section);
      const cell=[...p.querySelectorAll('.matrix-cell')].find(n=>rows[+n.dataset.row].cells[tab.columns[+n.dataset.column].sub]?.tooltip?.input?.chunks?.length);
      cell.focus();window.__buildTooltip(cell);
      return cell._ttip.querySelectorAll('.ttip-chunks tr').length;
    """)
    assert chunks > 1


def test_native_picker_focus_schema_and_reference_disclosure(matrix_driver, rendered_page):
    d = matrix_driver
    load(d, rendered_page, "?vertical=1")
    version = d.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-slot-version='0']")
    previous = version.get_attribute("value")
    version.send_keys(Keys.ARROW_DOWN)
    assert d.switch_to.active_element.get_attribute("data-slot-version") == "0"
    assert d.switch_to.active_element.get_attribute("value") != previous
    reference = d.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-slot-reference='2']")
    reference.send_keys(Keys.SPACE)
    assert d.switch_to.active_element.get_attribute("data-slot-reference") == "2"
    chip = d.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip")[1]
    group = chip.get_attribute("data-group")
    chip.send_keys(Keys.ENTER)
    assert d.switch_to.active_element.get_attribute("data-group") == group
    assert d.switch_to.active_element.get_attribute("aria-pressed") == "false"
    trigger = d.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-card [data-slot-picker='0']")
    trigger.send_keys(Keys.ENTER)
    engine = d.find_element(By.CSS_SELECTOR, "dialog[open] [data-picker-engine]")
    engine.send_keys(Keys.END)
    d.find_element(By.CSS_SELECTOR, "dialog[open] [data-picker-apply]").send_keys(Keys.ENTER)
    assert d.switch_to.active_element.get_attribute("data-slot-picker") == "0"
    assert "None" in d.switch_to.active_element.text
    click(d, ".legacy-report-details > summary")
    assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active table[data-parity-table]").is_displayed()
    header = d.execute_script("""
      const p=document.querySelector('.tab-panel.active'),t=window.__CONFORMANCE_PAGE.tabs.find(t=>t.id===p.id);
      return [...p.querySelectorAll('.matrix-case-heading')].find(n=>t.columns[+n.dataset.caseColumn].schemas?.length);
    """)
    assert header is not None
    d.execute_script("arguments[0].focus()", header)
    WebDriverWait(d, 10).until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".ttip-visible .case-schema-toggle"))
    schema = d.find_element(By.CSS_SELECTOR, ".ttip-visible .case-schema-toggle")
    schema.send_keys(Keys.ENTER)
    assert d.find_element(By.CSS_SELECTOR, ".ttip-visible details.case-schema").get_dom_attribute("open") is not None
    assert d.find_element(By.CSS_SELECTOR, ".ttip-visible .case-schema-json").is_displayed()
    d.execute_script("document.querySelector('.tab-panel.active [data-focus=disagreements]').click()")
    assert not d.find_elements(By.CSS_SELECTOR, ".ttip-visible")
    assert d.find_element(By.CSS_SELECTOR, ".tab-panel.active table[data-parity-table]").is_displayed()


def test_first_paint_never_shows_reference_table(matrix_driver, rendered_page):
    d = matrix_driver
    script = """
      window.__firstTable=null;window.__legacyPaint=false;
      function record() {
        const p=document.querySelector('.tab-panel.active');if(!p)return;
        const visible=n=>!n.closest('details:not([open])')&&getComputedStyle(n).display!=='none'&&getComputedStyle(n).visibility!=='hidden'&&n.getClientRects().length;
        for(const t of p.querySelectorAll('table[data-parity-table],.comparison-matrix')) {
          if(visible(t)){if(!window.__firstTable)window.__firstTable=t.className;if(t.hasAttribute('data-parity-table'))window.__legacyPaint=true;}
        }
      }
      new MutationObserver(record).observe(document,{childList:true,subtree:true,attributes:true});
    """
    registration = d.execute_cdp_cmd("Page.addScriptToEvaluateOnNewDocument", {"source": script})
    try:
        d.execute_cdp_cmd("Emulation.setCPUThrottlingRate", {"rate": 4})
        for query in ("", "?transpose=1", "?vertical=1", "?base_tab-unified=vllm_rust&cmp_tab-unified=vllm_rust"):
            load(d, rendered_page, query)
            assert d.execute_script("return window.__firstTable") == "comparison-matrix"
            assert d.execute_script("return window.__legacyPaint") is False
    finally:
        d.execute_cdp_cmd("Emulation.setCPUThrottlingRate", {"rate": 1})
        d.execute_cdp_cmd("Page.removeScriptToEvaluateOnNewDocument", {"identifier": registration["identifier"]})
