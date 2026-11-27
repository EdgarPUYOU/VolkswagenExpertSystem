# Customer-to-model matching and lead intent (ported from VW_sales_rules.ttl, SWRL Rule4 to Rule13).

PREFIX vw: <http://example.org/volkswagen/>

# Rule4: model price within the client's maximum budget
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?client vw:withinBudget ?model } }
WHERE {
  ?client a vw:Client ; vw:maxBudget ?budget .
  ?model a vw:Model ; vw:price ?price .
  FILTER(?price <= ?budget)
} ;

# Rule5: model has enough seats
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?client vw:meetsSeatingRequirement ?model } }
WHERE {
  ?client a vw:Client ; vw:minimumSeats ?minimum .
  ?model a vw:Model ; vw:seats ?seatCount .
  FILTER(?seatCount >= ?minimum)
} ;

# Rule6: model has the preferred powertrain
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?client vw:matchesPowertrain ?model } }
WHERE {
  ?client a vw:Client ; vw:preferredPowertrain ?powertrain .
  ?model a vw:Model ; vw:hasPowertrain ?powertrain .
} ;

# Rule7: suitable model = within budget, enough seats, preferred powertrain
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?client vw:suitableModel ?model } }
WHERE {
  ?client a vw:Client ;
          vw:withinBudget ?model ;
          vw:meetsSeatingRequirement ?model ;
          vw:matchesPowertrain ?model .
} ;

# Rule8: model has the preferred vehicle type
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?client vw:matchesVehicleType ?model } }
WHERE {
  ?client vw:preferredVehicleType ?type .
  ?model a vw:Model ; vw:hasVehicleType ?type .
} ;

# Rule9: model matches a stated customer interest
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?customer vw:matchesCustomerInterest ?model } }
WHERE {
  ?customer vw:hasInterest ?interest .
  ?model a vw:Model ; vw:modelMatchesInterest ?interest .
} ;

# Rule10: model supports a stated vehicle use
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?customer vw:matchesVehicleUse ?model } }
WHERE {
  ?customer vw:usesVehicleFor ?use .
  ?model a vw:Model ; vw:modelSupportsUse ?use .
} ;

# Rule11: likely interest from stated preferences (requires personalisation consent)
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?customer vw:likelyInterestedIn ?model } }
WHERE {
  ?customer a vw:PotentialCustomer ;
            vw:hasPersonalizationConsent ?consent ;
            vw:suitableModel ?model ;
            vw:matchesVehicleType ?model ;
            vw:matchesCustomerInterest ?model .
  FILTER(?consent = true)
} ;

# Rule12: likely interest from stated vehicle use (requires personalisation consent)
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?customer vw:likelyInterestedIn ?model } }
WHERE {
  ?customer a vw:PotentialCustomer ;
            vw:hasPersonalizationConsent ?consent ;
            vw:suitableModel ?model ;
            vw:matchesVehicleType ?model ;
            vw:matchesVehicleUse ?model .
  FILTER(?consent = true)
} ;

# Rule13: high-intent lead = likely interest, ready to purchase, within three months
INSERT { GRAPH <http://example.org/volkswagen/graph/inferred> { ?customer vw:highIntentForModel ?model } }
WHERE {
  ?customer vw:likelyInterestedIn ?model ;
            vw:buyingStage vw:ReadyToPurchase ;
            vw:purchaseHorizon vw:WithinThreeMonths .
}
