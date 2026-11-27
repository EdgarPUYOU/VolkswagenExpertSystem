#!/usr/bin/env python3
"""Generate VW_Supply_Abox.ttl and patch VW_Contracts_*.ttl for the supply-terms module.

Run from the repo root:  python3 tools/gen_supply_abox.py

The data tables below are the source of truth for the illustrative supply terms.
The script is safe to run again: it rewrites VW_Supply_Abox.ttl and only patches the
contract files when they have not been patched yet.
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# contract number -> (component local name, supplier country) is read from the A-box; the tables below
# only hold the terms that are new.

# Contract end dates (varied so the expiry question has a real answer).
END_DATES = {
    1: "2027-06-30", 2: "2028-12-31", 3: "2027-05-31", 4: "2028-06-30", 5: "2028-12-31", 6: "2027-03-31",
    7: "2029-06-30", 8: "2028-03-31", 9: "2027-12-31", 10: "2027-09-30", 11: "2029-03-31", 12: "2028-09-30",
    13: "2028-12-31", 14: "2027-08-31", 15: "2029-12-31", 16: "2028-06-30", 17: "2027-04-30", 18: "2029-06-30",
    19: "2028-03-31", 20: "2028-12-31", 21: "2027-10-31", 22: "2027-03-31", 23: "2028-09-30", 24: "2027-12-31",
}
CURRENCY_BY_COUNTRY = {"Germany": "EUR", "Sweden": "SEK", "Czech Republic": "CZK"}

# Indexation clauses: contract number -> [(index, weight, pass-through)]. Contracts missing here
# (3, 8, 14, 17, 18, 19, 20, 21) have fixed prices.
CLAUSES = {
    1: [("Steel", .25, 1.0), ("Aluminium", .20, 1.0)],
    2: [("Copper", .30, 1.0), ("Aluminium", .10, 1.0)],
    4: [("Steel", .25, .8), ("Aluminium", .20, .8)],
    5: [("Copper", .30, 1.0), ("Aluminium", .10, 1.0)],
    6: [("Steel", .30, 1.0), ("Aluminium", .15, 1.0)],
    7: [("Steel", .12, 1.0)],
    9: [("Steel", .12, 1.0)],
    10: [("Lithium", .45, 1.0), ("Copper", .10, 1.0), ("Aluminium", .05, 1.0), ("EnergyEU", .05, 1.0)],
    11: [("Lithium", .45, .75), ("Copper", .10, .75), ("EnergyEU", .05, .75)],
    12: [("Lithium", .45, 1.0), ("Copper", .10, 1.0), ("EnergyEU", .05, 1.0)],
    13: [("Steel", .30, 1.0)],
    15: [("Steel", .35, 1.0)],
    16: [("Steel", .35, 1.0), ("Aluminium", .10, 1.0), ("EnergyEU", .05, 1.0)],
    22: [("NaturalRubber", .40, 1.0), ("Steel", .08, 1.0), ("EnergyEU", .05, 1.0)],
    23: [("NaturalRubber", .38, 1.0), ("EnergyEU", .05, 1.0)],
    24: [("NaturalRubber", .42, 1.0), ("Steel", .10, 1.0), ("EnergyEU", .05, 1.0)],
}

# Sourcing: contract number -> (type, alternative supplier or None)
SOURCING = {n: ("SingleSource", None) for n in range(1, 25)}
SOURCING.update({
    1: ("DualSource", "Supplier_AltPowertrain"), 4: ("DualSource", "Supplier_AltPowertrain"),
    7: ("MultiSource", "Supplier_AltSeating"), 13: ("DualSource", "Supplier_AltTransmission"),
    14: ("DualSource", "Supplier_AltTransmission"), 16: ("DualSource", "Supplier_AltBrakes"),
    19: ("DualSource", "Supplier_AltInfotainment"), 22: ("MultiSource", "Supplier_AltTyres"),
    23: ("DualSource", "Supplier_AltTyres"),
})

PAYMENT_BY_COUNTRY = {"Germany": "Net60", "Sweden": "Net30", "Czech Republic": "Net90"}

ALT_SUPPLIERS = [
    ("Supplier_AltPowertrain", "Illustrative Alternative Powertrain Supplier", "Poland", 65),
    ("Supplier_AltTransmission", "Illustrative Alternative Transmission Supplier", "Hungary", 60),
    ("Supplier_AltBrakes", "Illustrative Alternative Brake Supplier", "Hungary", 50),
    ("Supplier_AltTyres", "Illustrative Alternative Tyre Supplier", "Poland", 40),
    ("Supplier_AltSeating", "Illustrative Alternative Seating Supplier", "Slovakia", 55),
    ("Supplier_AltInfotainment", "Illustrative Alternative Infotainment Supplier", "Hungary", 80),
]

# Illustrative planned launches/refreshes for the models fed by battery contracts.
LAUNCHES = {"ID4": "2027-03-15", "Enyaq": "2027-04-01", "Taycan": "2027-06-01", "e-tron": "2027-09-01"}


def contract_blocks(text):
    """Yield (number, name, block) for each SupplyContract in the contracts A-box."""
    for m in re.finditer(r"^(vw:SupplyContract_(\d\d)_\w+) a vw:SupplyContract ;.*?\.\n", text, re.S | re.M):
        yield int(m.group(2)), m.group(1), m.group(0)


def supplier_countries(text):
    """Map supplier local name -> country from the contracts A-box."""
    out = {}
    for m in re.finditer(r"^vw:(\w+) a [^.]*?vw:supplierCountry \"([^\"]+)\"", text, re.S | re.M):
        out[m.group(1)] = m.group(2)
    return out


def patch_contracts(abox_path, countries):
    text = abox_path.read_text()
    if 'vw:contractCurrency "EUR"' not in text and 'vw:contractCurrency vw:EUR' in text:
        return False  # already patched
    out = text
    for number, name, block in contract_blocks(text):
        supplier = re.search(r"vw:contractSupplier vw:(\w+)", block).group(1)
        currency = CURRENCY_BY_COUNTRY[countries[supplier]]
        new = re.sub(r'vw:contractCurrency "[A-Z]+"', f"vw:contractCurrency vw:{currency}", block)
        new = re.sub(r'vw:contractEndDate "[\d-]+"\^\^xsd:date', f'vw:contractEndDate "{END_DATES[number]}"^^xsd:date', new)
        assert new != block, name
        out = out.replace(block, new)
    abox_path.write_text(out)
    return True


def main():
    contracts_abox = ROOT / "VW_Contracts_Abox.ttl"
    text = contracts_abox.read_text()
    countries = supplier_countries(text)
    patched = patch_contracts(contracts_abox, countries)
    text = contracts_abox.read_text()

    lines = [
        "@prefix vw:   <http://example.org/volkswagen/> .",
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .",
        "@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .",
        "",
        "# Illustrative supply terms. Generated by tools/gen_supply_abox.py; edit the tables there, not this file.",
        "",
        "# Sourcing types and payment terms",
        'vw:SingleSource a vw:SourcingType ; rdfs:label "Single source" .',
        'vw:DualSource a vw:SourcingType ; rdfs:label "Dual source" .',
        'vw:MultiSource a vw:SourcingType ; rdfs:label "Multi source" .',
        'vw:Net30 a vw:PaymentTerm ; rdfs:label "Net 30" ; vw:paymentDays 30 .',
        'vw:Net60 a vw:PaymentTerm ; rdfs:label "Net 60" ; vw:paymentDays 60 .',
        'vw:Net90 a vw:PaymentTerm ; rdfs:label "Net 90" ; vw:paymentDays 90 .',
        "",
        "# Qualified alternative suppliers (fictional)",
    ]
    for iri, label, country, lead in ALT_SUPPLIERS:
        lines += [f"vw:{iri} a vw:Supplier ;", f'    rdfs:label "{label}" ;',
                  f'    vw:supplierCountry "{country}" ;', f"    vw:supplierLeadTimeDays {lead} .", ""]

    lines.append("# Per-contract terms and indexation clauses")
    for number, name, block in contract_blocks(text):
        supplier = re.search(r"vw:contractSupplier vw:(\w+)", block).group(1)
        stype, alt = SOURCING[number]
        clauses = CLAUSES.get(number, [])
        local = name.removeprefix("vw:")
        props = [f"    vw:sourcingType vw:{stype}", f"    vw:hasPaymentTerm vw:{PAYMENT_BY_COUNTRY[countries[supplier]]}"]
        if alt:
            props.append(f"    vw:alternativeSupplier vw:{alt}")
        suffix_names = [f"vw:Clause_{number:02d}_{idx}" for idx, _, _ in clauses]
        if suffix_names:
            props.append("    vw:hasIndexationClause " + ", ".join(suffix_names))
        lines.append(f"{name}\n" + " ;\n".join(props) + " .")
        for (idx, w, p), clause in zip(clauses, suffix_names):
            lines.append(f"{clause} a vw:IndexationClause ;\n    vw:indexedTo vw:Index_{idx} ;\n"
                         f'    vw:indexWeight "{w:.2f}"^^xsd:decimal ;\n    vw:passThroughRatio "{p:.2f}"^^xsd:decimal .')
        lines.append("")

    lines.append("# Illustrative planned launches")
    for model, date in LAUNCHES.items():
        lines.append(f'vw:{model} vw:plannedLaunchDate "{date}"^^xsd:date .')
    lines.append("")
    (ROOT / "VW_Supply_Abox.ttl").write_text("\n".join(lines))
    print(f"wrote VW_Supply_Abox.ttl ({sum(len(v) for v in CLAUSES.values())} clauses); contracts patched: {patched}")


if __name__ == "__main__":
    main()
