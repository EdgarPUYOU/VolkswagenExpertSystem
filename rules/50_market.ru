# Market rules: models that depend on a subsidy, and cars that sit above a CO2 fleet target.

PREFIX vw: <http://example.org/volkswagen/>

INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?model a vw:SubsidyDependentModel ;
           vw:subsidisedBy ?reg .
  }
}
WHERE {
  ?reg a vw:EVSubsidy ; vw:appliesToPowertrain ?pt ; vw:appliesInMarket ?market .
  ?model a vw:Model ; vw:hasPowertrain ?pt ; vw:soldInMarket ?market .
} ;

INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?model a vw:CO2PenaltyRiskModel ;
           vw:co2TargetBreachedUnder ?reg .
  }
}
WHERE {
  ?reg a vw:CO2FleetTarget ; vw:targetGramsPerKm ?target ; vw:appliesInMarket ?market .
  ?model a vw:Model ; vw:co2GramsPerKm ?g ; vw:soldInMarket ?market .
  FILTER(?g > ?target)
}
