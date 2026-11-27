# Generic materialisation: what an OWL/RDFS reasoner would derive from the TBox.
# Every operation inserts only triples that are not already visible, into the inferred graph.
# The engine runs all rule files repeatedly until nothing new appears, so order does not matter.

PREFIX rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
PREFIX owl:  <http://www.w3.org/2002/07/owl#>
PREFIX vw:   <http://example.org/volkswagen/>

# rdfs:subPropertyOf (hasEngineModel -> hasComponent, hasModel -> hasProduct -> hasOffering, ...)
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?s ?q ?o } }
WHERE {
  ?p rdfs:subPropertyOf+ ?q .
  ?s ?p ?o .
  FILTER NOT EXISTS { ?s ?q ?o }
} ;

# owl:inverseOf (hasModel/modelOf, hasBrand/belongsToGroup, contractCoversComponent/componentHasSupplyContract)
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?o ?q ?s } }
WHERE {
  { ?p owl:inverseOf ?q } UNION { ?q owl:inverseOf ?p }
  ?s ?p ?o .
  FILTER(isIRI(?o))
  FILTER NOT EXISTS { ?o ?q ?s }
} ;

# rdfs:subClassOf: an instance of a class is an instance of every superclass
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?s a ?c } }
WHERE {
  ?s a ?x .
  ?x rdfs:subClassOf+ ?c .
  FILTER(isIRI(?c))
  FILTER NOT EXISTS { ?s a ?c }
} ;

# Defined classes (owl:equivalentClass with a restriction), membership direction only.
# ElectricModel = Model with powertrain BatteryElectric
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?m a vw:ElectricModel } }
WHERE {
  ?m a vw:Model ; vw:hasPowertrain vw:BatteryElectric .
  FILTER NOT EXISTS { ?m a vw:ElectricModel }
} ;

# HybridModel = Model with powertrain PlugInHybrid
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?m a vw:HybridModel } }
WHERE {
  ?m a vw:Model ; vw:hasPowertrain vw:PlugInHybrid .
  FILTER NOT EXISTS { ?m a vw:HybridModel }
} ;

# CombustionModel = Model with powertrain Petrol or Diesel
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?m a vw:CombustionModel } }
WHERE {
  ?m a vw:Model ; vw:hasPowertrain ?pt .
  VALUES ?pt { vw:Petrol vw:Diesel }
  FILTER NOT EXISTS { ?m a vw:CombustionModel }
} ;

# BrandWithModels = Brand with some Model
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?b a vw:BrandWithModels } }
WHERE {
  ?b a vw:Brand ; vw:hasModel ?m .
  ?m a vw:Model .
  FILTER NOT EXISTS { ?b a vw:BrandWithModels }
} ;

# BrandWithElectricModels = Brand with some ElectricModel
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?b a vw:BrandWithElectricModels } }
WHERE {
  ?b a vw:Brand ; vw:hasModel ?m .
  ?m a vw:ElectricModel .
  FILTER NOT EXISTS { ?b a vw:BrandWithElectricModels }
} ;

# ModelsWithProductionSite = Model produced at some ProductionSite
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?m a vw:ModelsWithProductionSite } }
WHERE {
  ?m a vw:Model ; vw:producedAt ?s .
  ?s a vw:ProductionSite .
  FILTER NOT EXISTS { ?m a vw:ModelsWithProductionSite }
}
