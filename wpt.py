# pip install beautifulsoup4
# pip install pillow
# pip install pixelmatch

import glob
import shutil
import threading
import logging
import os
import subprocess
from urllib.parse import urljoin, urlparse
from typing import Optional
from pathlib import Path

from bs4 import BeautifulSoup
from PIL import Image, ImageFile
from pixelmatch.contrib.PIL import pixelmatch

ImageFile.LOAD_TRUNCATED_IMAGES = True

# Set up logging
logger = logging.getLogger("taffy-wpt")
logger.setLevel(logging.INFO)
console_handler = logging.StreamHandler()
console_handler.setLevel(logging.INFO)
formatter = logging.Formatter("%(asctime)s - %(name)s - %(levelname)s - %(message)s")
console_handler.setFormatter(formatter)
logger.addHandler(console_handler)

def serve_directory(wpt_dir, port=8000):
    subprocess.run(
        f"python -m http.server {port} --directory {wpt_dir}",
        shell=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL
    )
    
def create_diff_image_or_none(img1, img2) -> Optional[Image.Image]:
    
    img_diff = Image.new("RGBA", img1.size)
    did_pass = pixelmatch(img1, img2, img_diff, threshold=0.0, diff_mask=True, includeAA=True) <= 0

    if not did_pass:
        return img_diff
    else:
        return None


def main():
    wpt_dir = os.getenv("WPT_DIR")

    if wpt_dir is None:
        logging.error("WPT_DIR environment variable is not set")
        quit()
    
    blitz_path = os.getenv("BLITZ_REPO")

    if blitz_path is None:
        logging.error("BLITZ_REPO environment variable is not set")
        quit()
        
    timeout = 5

    output_path = f"{blitz_path}/examples/output/localhost800.png"
    
    # Start the server in a separate thread
    server_thread = threading.Thread(target=serve_directory, args=(wpt_dir, 8000))
    server_thread.start()

    shutil.rmtree("out", ignore_errors=True)
    os.makedirs("out", exist_ok=True)

    files = list(glob.glob(wpt_dir + "/css/css-flexbox/**/*.html", recursive=True))

    for file in files:
        fail_message = "FAIL: " + file
        pass_message = "PASS: " + file
        skip_message = "SKIP: " + file

        if file.endswith("-ref.html") or file.endswith(".tentative.html") or "reference" in file:
            continue

        rel_path = os.path.relpath(file, wpt_dir)
        url = f"http://localhost:8000/{rel_path}"
        cmd = ["cargo", "run", "--release", "--manifest-path", f"{blitz_path}/Cargo.toml", "--example", "screenshot", url]

        try:
            subprocess.run(cmd, timeout=timeout, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except subprocess.TimeoutExpired:
            logger.info(skip_message)
            continue
        except subprocess.CalledProcessError as e:
            print(e)
            logger.info(skip_message)
            continue

        actual_out_path = f"out/{rel_path}-act.png";
        Path(actual_out_path).parent.mkdir(parents=True, exist_ok=True)
        actual_image = Image.open(output_path)
        actual_image.save(actual_out_path)
        actual_image = Image.open(actual_out_path)

        with open(file, "r") as f:
            soup = BeautifulSoup(f, "html.parser")
        link_tag = soup.find("link", rel="match")

        if link_tag:
            href_value = link_tag["href"]
        else:
            logger.info(skip_message)
            continue
        
        url_dir = os.path.dirname(urlparse(url).path)
        # reference_url = "http://localhost:8000/css/css-flexbox" + urljoin(url_dir, href_value)
        reference_url = "http://localhost:8000/css/css-flexbox/" + href_value

        print(reference_url);
        
        cmd = ["cargo", "run", "--release", "--manifest-path", f"{blitz_path}/Cargo.toml", "--example", "screenshot", reference_url]

        try:
            subprocess.run(cmd, timeout=timeout, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except subprocess.TimeoutExpired:
            logger.info(skip_message)
            continue
        except subprocess.CalledProcessError as e:
            print(e)
            logger.info(skip_message)
            continue

        expected_out_path = f"out/{rel_path}-ref.png";
        Path(expected_out_path).parent.mkdir(parents=True, exist_ok=True)
        expected_image = Image.open(output_path)
        expected_image.save(expected_out_path)
        expected_image = Image.open(expected_out_path)

        rel_path = rel_path.replace("\\", "_")
        diff_output_path = f"out/{rel_path}-diff.png"
        diff_image = create_diff_image_or_none(actual_image, expected_image)

        are_equal = True
        if diff_image is not None:
            diff_image.save(diff_output_path)
            are_equal = False

        status = "pass" if are_equal else "fail"
        if are_equal:
            logger.info(pass_message)
        else:
            logger.error(fail_message)

    quit()

if __name__ == "__main__":
    main()
