# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Browser checks for the model-driven comparison layout."""
from __future__ import annotations

import shutil
from pathlib import Path

import pytest

selenium = pytest.importorskip("selenium")
from selenium import webdriver  # noqa: E402
from selenium.webdriver.chrome.options import Options  # noqa: E402
from selenium.webdriver.common.by import By  # noqa: E402
from selenium.webdriver.common.keys import Keys  # noqa: E402
from selenium.webdriver.support.ui import WebDriverWait  # noqa: E402

pytestmark = pytest.mark.skipif(
    not any(shutil.which(binary) for binary in ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser")),
    reason="no headless Chrome available",
)


def test_comparison_layout_tabs_filters_drawer_and_narrow_view(rendered_page: Path) -> None:
    options = Options()
    for argument in ("--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage", "--window-size=1600,1200"):
        options.add_argument(argument)
    driver = webdriver.Chrome(options=options)
    wait = WebDriverWait(driver, 15)
    page_url = rendered_page.resolve().as_uri()
    try:
        driver.get(page_url)
        wait.until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .comparison-matrix"))
        assert driver.find_element(By.CSS_SELECTOR, ".tab-button.active").get_attribute("data-tab-target") == "tab-unified"
        assert "matches GOLDEN" in driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text
        assert not driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-peer-only]").is_selected()
        assert driver.find_element(By.CSS_SELECTOR, ".legacy-report-details").get_dom_attribute("open") is None
        statuses = driver.execute_script("return [...new Set([...document.querySelectorAll('.tab-panel .matrix-status')].map(node => node.className.match(/status-([a-z]+)/)?.[1]))].sort();")
        assert {"pass", "documented", "diverge", "leak", "error", "expected", "missing", "unsupported"} <= set(statuses), statuses
        bar_widths = driver.execute_script("return [...document.querySelectorAll('.tab-panel.active .engine-status-bar')].map(bar => [...bar.querySelectorAll('span')].reduce((total, node) => total + parseFloat(node.style.width), 0));")
        assert bar_widths and all(width == pytest.approx(100) for width in bar_widths), bar_widths

        for tab_id in ("tab-toolcalling-batch", "tab-toolcalling-streamv1", "tab-reasoning-batch", "tab-reasoning-stream", "tab-unified"):
            driver.get(page_url + "?tab=" + tab_id)
            wait.until(lambda browser, requested=tab_id: browser.find_element(
                By.CSS_SELECTOR, ".tab-button.active"
            ).get_attribute("data-tab-target") == requested)

        for query in (
            "?tab=tab-unified&view=details",
            "?tab=tab-unified&transpose=0",
            "?tab=tab-unified&transpose=1",
            "?tab=tab-unified&base_tab-unified=vllm_rust&cmp_tab-unified=vllm_rust",
        ):
            driver.get(page_url + query)
            wait.until(lambda browser: browser.find_element(
                By.CSS_SELECTOR, ".legacy-report-details"
            ).get_dom_attribute("open") is not None)
        assert driver.find_element(By.CSS_SELECTOR, "#tab-unified input.cmp-ref:checked").get_attribute("value") == "vllm_rust"

        driver.get(page_url)
        wait.until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell"))
        search = driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-case-search]")
        search.send_keys("tool_only")
        assert driver.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell")
        search.send_keys(Keys.CONTROL, "a", Keys.BACKSPACE)
        driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-layout='stacked']").click()
        assert "layout-stacked" in driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .comparison-ui").get_attribute("class")
        score_before_focus = driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text
        driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-focus='problems']").click()
        assert driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text == score_before_focus
        problem_cells = driver.execute_script("return [...document.querySelectorAll('.tab-panel.active .matrix-cell')].filter(cell => [...cell.querySelectorAll('.matrix-status')].some(status => ['expected','missing','unsupported'].some(kind => status.classList.contains('status-' + kind)))).map(cell => cell.className);")
        assert problem_cells and all("focus-dim" not in class_name for class_name in problem_cells), problem_cells

        cell = driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell")
        cell.click()
        wait.until(lambda browser: "open" in browser.find_element(By.CSS_SELECTOR, ".comparison-drawer").get_attribute("class"))
        drawer = driver.find_element(By.CSS_SELECTOR, ".drawer-panel")
        wait.until(lambda browser: browser.execute_script(
            "return document.querySelector('.drawer-panel').getBoundingClientRect().right <= document.documentElement.clientWidth + 1"
        ))
        bounds = driver.execute_script("const r=document.querySelector('.drawer-panel').getBoundingClientRect();return {left:r.left,right:r.right,width:r.width}")
        assert bounds["left"] >= 0 and bounds["right"] <= driver.execute_script("return document.documentElement.clientWidth + 1"), bounds
        assert driver.find_element(By.CSS_SELECTOR, "[data-drawer-content]").text
        driver.switch_to.active_element.send_keys(Keys.ESCAPE)
        wait.until(lambda browser: "open" not in browser.find_element(By.CSS_SELECTOR, ".comparison-drawer").get_attribute("class"))

        driver.get(page_url + "?tab=tab-toolcalling-batch")
        wait.until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell"))
        before = len(driver.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell"))
        driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-peer-only]").click()
        after = len(driver.find_elements(By.CSS_SELECTOR, ".tab-panel.active .matrix-cell"))
        assert 0 < after <= before

        version = driver.find_element(By.CSS_SELECTOR, ".tab-panel.active [data-candidate-select='dynamo']")
        driver.execute_script("arguments[0].value='dynamo_v1-b-9-2-13';arguments[0].dispatchEvent(new Event('change',{bubbles:true}));", version)
        assert "dynamo_v1-b-9-2-13" in driver.current_url

        driver.get(page_url)
        wait.until(lambda browser: browser.find_elements(By.CSS_SELECTOR, ".tab-panel.active .category-chip"))
        driver.execute_script("while (true) { const chip=document.querySelector('.tab-panel.active .category-chip[aria-pressed=true]'); if (!chip) break; chip.click(); }")
        assert driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text.startswith("—")
        assert "0 captured" in driver.find_element(By.CSS_SELECTOR, ".tab-panel.active .engine-score").text

        theme_button = driver.find_element(By.CSS_SELECTOR, "[data-theme-toggle]")
        before_theme = driver.execute_script("return document.documentElement.dataset.theme")
        theme_button.click()
        assert driver.execute_script("return document.documentElement.dataset.theme") != before_theme
        driver.set_window_size(540, 900)
        geometry = driver.execute_script("return {width:innerWidth, documentWidth:document.documentElement.scrollWidth, card:document.querySelector('.tab-panel.active .engine-card').getBoundingClientRect().width}")
        assert geometry["card"] > 0
        assert geometry["documentWidth"] <= geometry["width"] + 1
        assert not [entry for entry in driver.get_log("browser") if entry["level"] == "SEVERE"]
    finally:
        driver.quit()
