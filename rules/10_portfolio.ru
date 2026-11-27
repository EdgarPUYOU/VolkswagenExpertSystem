# Portfolio and production-site rules (ported from VWGroup_rules.ttl, SWRL Rule1 to Rule3).

PREFIX vw: <http://example.org/volkswagen/>

# ASSUMPTION (illustrative data): the A-box has no vw:producedAt facts, so a model with no
# explicit production site is taken to be produced at its brand's production site.
# Remove this rule once real model-to-site facts are added.
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?m vw:producedAt ?site } }
WHERE {
  ?b a vw:Brand ; vw:hasModel ?m ; vw:hasProductionSite ?site .
  FILTER NOT EXISTS { ?m vw:producedAt ?any }
} ;

# Rule1: models belonging to a brand group
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?g vw:groupHasModel ?m } }
WHERE {
  ?g a vw:BrandGroup ; vw:hasBrand ?b .
  ?b vw:hasModel ?m .
} ;

# Rule2: sites used by a brand for its models
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?b vw:brandHasModelAtSite ?s } }
WHERE {
  ?b a vw:Brand ; vw:hasModel ?m .
  ?m vw:producedAt ?s .
} ;

# Rule3: sites used by a brand group
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?g vw:groupUsesSite ?s } }
WHERE {
  ?g a vw:BrandGroup ; vw:hasBrand ?b .
  ?b vw:brandHasModelAtSite ?s .
}
