# Exposure rules: which contracts, models and profit centers are exposed to which indices and currencies,
# and which components have a single source with no qualified alternative.

PREFIX vw: <http://example.org/volkswagen/>

# A contract with an indexation clause is exposed to the index it is indexed to
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?contract a vw:CommodityExposedContract ;
              vw:exposedToIndex ?index .
  }
}
WHERE {
  ?contract a vw:SupplyContract ;
            vw:hasIndexationClause ?clause .
  ?clause vw:indexedTo ?index .
} ;

# A model is exposed to the indices of the contracts covering its components
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?model a vw:CommodityExposedModel ;
           vw:exposedToIndex ?index .
  }
}
WHERE {
  ?model a vw:Model ; vw:hasComponent ?component .
  ?contract vw:contractCoversComponent ?component ;
            vw:exposedToIndex ?index .
} ;

# A profit center is exposed to the indices of its brand's models
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?pc a vw:CommodityExposedProfitCenter ;
        vw:exposedToIndex ?index .
  }
}
WHERE {
  ?brand vw:brandProfitCenter ?pc ;
         vw:hasModel ?model .
  ?model vw:exposedToIndex ?index .
} ;

# A contract invoiced in a non-reporting currency is exposed to that currency
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?contract a vw:FXExposedContract ;
              vw:exposedToCurrency ?currency .
  }
}
WHERE {
  ?contract a vw:SupplyContract ;
            vw:contractCurrency ?currency .
  ?currency vw:isReportingCurrency false .
} ;

# A single-sourced contract with no alternative supplier makes its component a critical single source
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?component a vw:SingleSourcedCriticalComponent } }
WHERE {
  ?contract a vw:SupplyContract ;
            vw:contractCoversComponent ?component ;
            vw:sourcingType vw:SingleSource .
  FILTER NOT EXISTS { ?contract vw:alternativeSupplier ?alt }
}
