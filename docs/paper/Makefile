# Build in a separate cache dir so the repo stays clean.
BUILD ?= $(or $(TMPDIR),/tmp)/vw-paper-build
pdf:
	latexmk -pdf -interaction=nonstopmode -outdir=$(BUILD) main.tex
	cp $(BUILD)/main.pdf main.pdf
clean:
	rm -rf $(BUILD)
slides:
	latexmk -pdf -interaction=nonstopmode -outdir=$(BUILD) slides.tex
	cp $(BUILD)/slides.pdf slides.pdf
